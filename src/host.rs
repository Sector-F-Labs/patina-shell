//! A layer-shell host for egui: wlr-layer-shell surfaces, each with its own
//! egui context, drawn in software into wl_shm buffers.
//!
//! eframe/winit only make xdg-shell windows, so desktop pieces such as the
//! bar and the launcher need a host of their own (ADR-0003). Nothing here is
//! specific to one of them: a [`Shell`] says what to draw, [`Options`] says
//! where, and the host does surfaces, outputs, scale, input and painting.
//!
//! With [`Options::lock`], the surfaces are ext-session-lock surfaces
//! instead, one per output, for the lock screen (ADR-0028).

use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{EventLoop, LoopHandle},
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        },
        protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
            Shape, WpCursorShapeDeviceV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};

pub use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use crate::raster::Painter;
use crate::scale::{FractionalData, NoEvents};
use crate::toplevels::{ManagerData, Toplevel, ToplevelRequest, Toplevels};
use smithay_client_toolkit::reexports::protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::WpFractionalScaleV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

/// What a host draws.
pub trait Shell: 'static {
    /// Called for each new egui context, before its first frame, e.g. to
    /// apply a skin.
    fn setup(&mut self, ctx: &egui::Context);
    /// Draws the surface on `output`, the compositor's name for it (the
    /// name dwl uses in its status, e.g. `HEADLESS-1`). It's empty for a
    /// surface the compositor placed ([`Options::per_output`] off).
    fn ui(&mut self, ui: &mut egui::Ui, output: &str);
    /// Whether the program is done, checked after every frame; the host
    /// returns from [`Host::run`] when it is. A locking host
    /// ([`Options::lock`]) unlocks the session first.
    fn finished(&self) -> bool {
        false
    }
    /// The open windows changed ([`Options::toplevels`] on).
    fn toplevels_changed(&mut self, _toplevels: &[Toplevel]) {}
    /// Windows to activate or close, collected during `ui`.
    fn take_toplevel_requests(&mut self) -> Vec<ToplevelRequest> {
        Vec::new()
    }
}

/// Where the host's surfaces go and what they take.
pub struct Options {
    /// The layer-shell namespace, which compositors use in their rules.
    pub namespace: String,
    pub layer: Layer,
    /// The edges the surface sticks to; none centres it.
    pub anchor: Anchor,
    /// Size in logical pixels; 0 stretches between opposite anchors.
    pub size: (u32, u32),
    /// Space reserved at the anchored edge, as a bar does; 0 for none.
    pub exclusive_zone: i32,
    /// Space between the surface and the edges it's anchored to, in
    /// logical pixels: top, right, bottom, left.
    pub margin: (i32, i32, i32, i32),
    pub keyboard: KeyboardInteractivity,
    /// One surface on every output (a bar), or a single one wherever the
    /// compositor puts it (a launcher).
    pub per_output: bool,
    /// Follow the open windows, through wlr-foreign-toplevel-management.
    pub toplevels: bool,
    /// Lock the session (ext-session-lock) and draw on a lock surface on
    /// every output, which the compositor sizes; the layer fields above
    /// are unused.
    pub lock: bool,
}

/// What a surface is to the compositor.
enum Role {
    Layer(LayerSurface),
    Lock(SessionLockSurface),
}

impl Role {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Role::Layer(l) => l.wl_surface(),
            Role::Lock(l) => l.wl_surface(),
        }
    }

    fn commit(&self) {
        match self {
            Role::Layer(l) => l.commit(),
            Role::Lock(l) => l.wl_surface().commit(),
        }
    }
}

struct Surface {
    output: Option<wl_output::WlOutput>,
    name: String,
    role: Role,
    ctx: egui::Context,
    painter: Painter,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    scale: i32,
    /// The exact scale, in 120ths, and the viewport that lets the buffer
    /// be drawn at it; without them, `scale`.
    scale_120: Option<u32>,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    configured: bool,
    dirty: bool,
    frame_pending: bool,
    repaint_at: Option<Instant>,
    events: Vec<egui::Event>,
    /// Whether this surface has the keyboard.
    focused: bool,
}

impl Drop for Surface {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}

pub struct Host<S: Shell> {
    pub shell: S,
    options: Options,
    conn: Connection,
    qh: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,
    registry_state: RegistryState,
    pub(crate) seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: Option<LayerShell>,
    /// The session lock, asked for at once when locking.
    lock: Option<SessionLock>,
    /// The lock ended without the shell finishing: refused, or taken away.
    lock_ended: Option<Result<(), String>>,
    shm: Shm,
    pool: SlotPool,
    cursor_shape: Option<CursorShapeManager>,
    pointer: Option<(wl_pointer::WlPointer, Option<WpCursorShapeDeviceV1>)>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: egui::Modifiers,
    surfaces: Vec<Surface>,
    start: Instant,
    pub(crate) toplevels: Toplevels,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
}

impl<S: Shell> Host<S> {
    /// Connects to the compositor and returns the host and an event loop
    /// with the Wayland connection in it. Add other sources (timers,
    /// channels) to the loop, then call [`Host::run`].
    pub fn connect(options: Options, shell: S) -> Result<(Self, EventLoop<'static, Self>), String> {
        let conn =
            Connection::connect_to_env().map_err(|e| format!("no Wayland compositor: {e}"))?;
        let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let event_loop = EventLoop::try_new().map_err(|e| e.to_string())?;
        WaylandSource::new(conn.clone(), queue)
            .insert(event_loop.handle())
            .map_err(|e| e.to_string())?;

        let compositor =
            CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
        let (layer_shell, lock) = if options.lock {
            let lock = SessionLockState::new(&globals, &qh).lock(&qh).map_err(|e| {
                format!("the compositor can't lock the session (ext_session_lock_manager_v1): {e}")
            })?;
            (None, Some(lock))
        } else {
            let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| {
                format!("the compositor has no layer shell (zwlr_layer_shell_v1): {e}")
            })?;
            (Some(layer_shell), None)
        };
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let pool = SlotPool::new(4096 * 32 * 4, &shm).map_err(|e| e.to_string())?;
        let mut host = Self {
            shell,
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state: OutputState::new(&globals, &qh),
            compositor,
            layer_shell,
            lock,
            lock_ended: None,
            shm,
            pool,
            cursor_shape: CursorShapeManager::bind(&globals, &qh).ok(),
            pointer: None,
            keyboard: None,
            modifiers: egui::Modifiers::default(),
            surfaces: Vec::new(),
            start: Instant::now(),
            toplevels: Toplevels::default(),
            fractional_manager: globals.bind(&qh, 1..=1, NoEvents).ok(),
            viewporter: globals.bind(&qh, 1..=1, NoEvents).ok(),
            loop_handle: event_loop.handle(),
            conn,
            qh,
            options,
        };
        if !host.options.per_output && !host.options.lock {
            host.add_surface(None);
        }
        if host.options.toplevels {
            host.toplevels.manager = globals.bind(&host.qh, 1..=3, ManagerData).ok();
        }
        Ok((host, event_loop))
    }

    /// Runs until the shell is [`Shell::finished`], or an error such as the
    /// compositor going away.
    pub fn run(mut self, mut event_loop: EventLoop<'static, Self>) -> Result<(), String> {
        loop {
            let timeout = self
                .next_repaint()
                .map(|t| t.saturating_duration_since(Instant::now()));
            event_loop
                .dispatch(timeout, &mut self)
                .map_err(|e| e.to_string())?;
            let now = Instant::now();
            for s in &mut self.surfaces {
                if s.repaint_at.is_some_and(|t| t <= now) {
                    s.repaint_at = None;
                    s.dirty = true;
                }
            }
            self.draw_dirty();
            self.send_toplevel_requests();
            if self.shell.finished() {
                self.unlock();
                return Ok(());
            }
            if let Some(ended) = self.lock_ended.take() {
                return ended;
            }
        }
    }

    /// Unlocks the session, if this host locked it. Only a finished shell
    /// does this: any other way out (an error, a crash) leaves the session
    /// locked, as the protocol wants.
    fn unlock(&mut self) {
        if let Some(lock) = self.lock.take() {
            lock.unlock();
            if let Err(e) = self.conn.flush() {
                eprintln!("patina-shell: unlocking: {e}");
            }
        }
    }

    /// Changes whether the surfaces take the keyboard: a notice that
    /// becomes a menu when the user opens it.
    pub fn set_keyboard(&mut self, keyboard: KeyboardInteractivity) {
        self.options.keyboard = keyboard;
        for s in &self.surfaces {
            if let Role::Layer(layer) = &s.role {
                layer.set_keyboard_interactivity(keyboard);
                layer.commit();
            }
        }
    }

    /// Redraws every surface, e.g. after the shell's state changed.
    pub fn redraw_all(&mut self) {
        for s in &mut self.surfaces {
            s.dirty = true;
        }
    }

    fn next_repaint(&self) -> Option<Instant> {
        self.surfaces.iter().filter_map(|s| s.repaint_at).min()
    }

    fn draw_dirty(&mut self) {
        for i in 0..self.surfaces.len() {
            let s = &self.surfaces[i];
            if s.dirty && s.configured && !s.frame_pending {
                self.draw(i);
            }
        }
    }

    fn draw(&mut self, i: usize) {
        let Self {
            shell,
            surfaces,
            pool,
            start,
            qh,
            modifiers,
            ..
        } = self;
        let s = &mut surfaces[i];
        s.dirty = false;
        let scale = s.scale.max(1);
        // The exact scale where the compositor gives one, else the integer.
        let ppp = match (s.scale_120, &s.viewport) {
            (Some(n), Some(_)) => n as f32 / 120.0,
            _ => scale as f32,
        };
        let (pw, ph) = (
            crate::scale::pixels(s.width, ppp),
            crate::scale::pixels(s.height, ppp),
        );
        if pw <= 0 || ph <= 0 {
            return;
        }

        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(s.width as f32, s.height as f32),
            )),
            time: Some(start.elapsed().as_secs_f64()),
            events: std::mem::take(&mut s.events),
            modifiers: *modifiers,
            focused: s.focused,
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        let out = s.ctx.run_ui(input, |ui| shell.ui(ui, &s.name));

        if let Some(v) = out.viewport_output.get(&egui::ViewportId::ROOT) {
            if v.repaint_delay.is_zero() {
                s.dirty = true;
            } else if v.repaint_delay < Duration::from_secs(3600) {
                s.repaint_at = Some(Instant::now() + v.repaint_delay);
            }
        }

        let primitives = s.ctx.tessellate(out.shapes, out.pixels_per_point);
        s.pixels.resize((pw * ph) as usize, 0);
        s.painter.paint(
            &mut s.pixels,
            pw as usize,
            ph as usize,
            out.pixels_per_point,
            &primitives,
            &out.textures_delta,
        );

        let (buffer, canvas) = match pool.create_buffer(pw, ph, pw * 4, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("patina-shell: no {pw}x{ph} buffer: {e}");
                return;
            }
        };
        for (dst, src) in canvas.chunks_exact_mut(4).zip(&s.pixels) {
            dst.copy_from_slice(&src.to_le_bytes());
        }
        let surface = s.role.wl_surface();
        match (&s.viewport, s.scale_120) {
            (Some(viewport), Some(_)) => {
                surface.set_buffer_scale(1);
                viewport.set_destination(s.width as i32, s.height as i32);
            }
            _ => surface.set_buffer_scale(scale),
        }
        surface.damage_buffer(0, 0, pw, ph);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("patina-shell: {e}");
            return;
        }
        s.role.commit();
        s.frame_pending = true;
    }

    fn add_surface(&mut self, output: Option<wl_output::WlOutput>) {
        let info = output.as_ref().and_then(|o| self.output_state.info(o));
        let name = info
            .as_ref()
            .and_then(|i| i.name.clone())
            .unwrap_or_default();
        let scale = info.as_ref().map_or(1, |i| i.scale_factor);
        let surface = self.compositor.create_surface(&self.qh);
        let fractional = self
            .fractional_manager
            .as_ref()
            .map(|m| m.get_fractional_scale(&surface, &self.qh, FractionalData));
        let viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&surface, &self.qh, NoEvents));
        let o = &self.options;
        // A lock surface waits for the compositor's size, as a layer
        // surface does for its configure.
        let (role, size) = match (&self.lock, &self.layer_shell, &output) {
            (Some(lock), _, Some(output)) => (
                Role::Lock(lock.create_lock_surface(surface, output, &self.qh)),
                (0, 0),
            ),
            (None, Some(layer_shell), _) => {
                let layer = layer_shell.create_layer_surface(
                    &self.qh,
                    surface,
                    o.layer,
                    Some(o.namespace.clone()),
                    output.as_ref(),
                );
                layer.set_anchor(o.anchor);
                layer.set_size(o.size.0, o.size.1);
                layer.set_exclusive_zone(o.exclusive_zone);
                layer.set_margin(o.margin.0, o.margin.1, o.margin.2, o.margin.3);
                layer.set_keyboard_interactivity(o.keyboard);
                layer.commit();
                (Role::Layer(layer), o.size)
            }
            _ => return,
        };

        let ctx = egui::Context::default();
        self.shell.setup(&ctx);
        self.surfaces.push(Surface {
            output,
            name,
            role,
            ctx,
            painter: Painter::default(),
            pixels: Vec::new(),
            width: size.0,
            height: size.1,
            scale,
            scale_120: None,
            fractional,
            viewport,
            configured: false,
            dirty: true,
            frame_pending: false,
            repaint_at: None,
            events: Vec::new(),
            focused: false,
        });
    }

    /// The compositor's exact scale for the surface behind `fractional`.
    pub(crate) fn set_scale_120(&mut self, fractional: &WpFractionalScaleV1, scale: u32) {
        if let Some(s) = self
            .surfaces
            .iter_mut()
            .find(|s| s.fractional.as_ref() == Some(fractional))
        {
            s.scale_120 = Some(scale);
            s.dirty = true;
        }
    }

    fn surface_mut(&mut self, surface: &wl_surface::WlSurface) -> Option<&mut Surface> {
        self.surfaces
            .iter_mut()
            .find(|s| s.role.wl_surface() == surface)
    }

    /// Hands a key to the focused surface.
    fn key(&mut self, event: &KeyEvent, pressed: bool, repeat: bool) {
        let modifiers = self.modifiers;
        let Some(s) = self.surfaces.iter_mut().find(|s| s.focused) else {
            return;
        };
        if let Some(key) = egui_key(event.keysym) {
            s.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat,
                modifiers,
            });
        }
        // Text, but not control characters or shortcuts.
        if pressed
            && !modifiers.ctrl
            && !modifiers.alt
            && let Some(text) = event
                .utf8
                .as_ref()
                .filter(|t| !t.chars().any(char::is_control))
        {
            s.events.push(egui::Event::Text(text.clone()));
        }
        s.dirty = true;
    }
}

/// The keys egui widgets act on. Letters and digits come through as text.
fn egui_key(sym: Keysym) -> Option<egui::Key> {
    use egui::Key;
    Some(match sym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::Tab | Keysym::ISO_Left_Tab => Key::Tab,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete => Key::Delete,
        Keysym::Up => Key::ArrowUp,
        Keysym::Down => Key::ArrowDown,
        Keysym::Left => Key::ArrowLeft,
        Keysym::Right => Key::ArrowRight,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        Keysym::Page_Up => Key::PageUp,
        Keysym::Page_Down => Key::PageDown,
        Keysym::space => Key::Space,
        // Letters and digits, for shortcuts (Ctrl+A, Ctrl+N, ...); their
        // text comes separately.
        _ => {
            let c = sym.key_char()?;
            if !c.is_ascii_alphanumeric() {
                return None;
            }
            return Key::from_name(&c.to_ascii_uppercase().to_string());
        }
    })
}

impl<S: Shell> CompositorHandler for Host<S> {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.scale = factor;
            s.dirty = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.frame_pending = false;
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl<S: Shell> OutputHandler for Host<S> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        // A lock covers every output, and is drawn before it's confirmed:
        // some compositors confirm only once every output shows it.
        if self.options.per_output || self.lock.is_some() {
            self.add_surface(Some(output));
        }
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let Some(info) = self.output_state.info(&output) else {
            return;
        };
        if let Some(s) = self
            .surfaces
            .iter_mut()
            .find(|s| s.output.as_ref() == Some(&output))
        {
            s.name = info.name.unwrap_or_default();
            s.dirty = true;
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces.retain(|s| s.output.as_ref() != Some(&output));
    }
}

impl<S: Shell> LayerShellHandler for Host<S> {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.surfaces
            .retain(|s| !matches!(&s.role, Role::Layer(l) if l == layer));
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(s) = self
            .surfaces
            .iter_mut()
            .find(|s| matches!(&s.role, Role::Layer(l) if l == layer))
        else {
            return;
        };
        let (w, h) = configure.new_size;
        if w > 0 {
            s.width = w;
        }
        if h > 0 {
            s.height = h;
        }
        s.configured = true;
        s.dirty = true;
    }
}

impl<S: Shell> SessionLockHandler for Host<S> {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {}

    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, lock: SessionLock) {
        self.lock_ended = Some(if lock.is_locked() {
            Ok(())
        } else {
            Err("the compositor didn't lock the session; is it locked already?".into())
        });
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _: u32,
    ) {
        let Some(s) = self.surface_mut(surface.wl_surface()) else {
            return;
        };
        let (w, h) = configure.new_size;
        s.width = w.max(1);
        s.height = h.max(1);
        s.configured = true;
        s.dirty = true;
    }
}

impl<S: Shell> SeatHandler for Host<S> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer if self.pointer.is_none() => {
                if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                    let device = self
                        .cursor_shape
                        .as_ref()
                        .map(|m| m.get_shape_device(&pointer, qh));
                    self.pointer = Some((pointer, device));
                }
            }
            // Bound even for surfaces that don't take the keyboard yet: one
            // may later (set_keyboard), and without focus it gets no keys.
            Capability::Keyboard if self.keyboard.is_none() => {
                let repeat = Box::new(
                    |host: &mut Self, _: &wl_keyboard::WlKeyboard, e: KeyEvent| {
                        host.key(&e, true, true);
                    },
                );
                self.keyboard = self
                    .seat_state
                    .get_keyboard_with_repeat(qh, &seat, None, self.loop_handle.clone(), repeat)
                    .ok();
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Some((pointer, device)) = self.pointer.take() {
                    if let Some(device) = device {
                        device.destroy();
                    }
                    pointer.release();
                }
            }
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl<S: Shell> KeyboardHandler for Host<S> {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.focused = true;
            s.events.push(egui::Event::WindowFocused(true));
            s.dirty = true;
        }
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.focused = false;
            s.events.push(egui::Event::WindowFocused(false));
            s.dirty = true;
        }
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(&event, true, false);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(&event, true, true);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(&event, false, false);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = egui::Modifiers {
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
            mac_cmd: false,
            command: m.ctrl,
        };
    }
}

impl<S: Shell> PointerHandler for Host<S> {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let pos = egui::pos2(event.position.0 as f32, event.position.1 as f32);
            let egui_event = match event.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some((_, Some(device))) = &self.pointer {
                        device.set_shape(serial, Shape::Default);
                    }
                    egui::Event::PointerMoved(pos)
                }
                PointerEventKind::Leave { .. } => egui::Event::PointerGone,
                PointerEventKind::Motion { .. } => egui::Event::PointerMoved(pos),
                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let Some(button) = egui_button(button) else {
                        continue;
                    };
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: matches!(event.kind, PointerEventKind::Press { .. }),
                        modifiers: self.modifiers,
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(-horizontal.absolute as f32, -vertical.absolute as f32),
                    phase: egui::TouchPhase::Move,
                    modifiers: self.modifiers,
                },
            };
            if let Some(s) = self.surface_mut(&event.surface) {
                s.events.push(egui_event);
                s.dirty = true;
            }
        }
    }
}

/// Linux evdev button codes, which Wayland uses on every system.
fn egui_button(code: u32) -> Option<egui::PointerButton> {
    match code {
        0x110 => Some(egui::PointerButton::Primary),
        0x111 => Some(egui::PointerButton::Secondary),
        0x112 => Some(egui::PointerButton::Middle),
        0x113 => Some(egui::PointerButton::Extra1),
        0x114 => Some(egui::PointerButton::Extra2),
        _ => None,
    }
}

impl<S: Shell> ShmHandler for Host<S> {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl<S: Shell> ProvidesRegistryState for Host<S> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(@<S: Shell> Host<S>);
smithay_client_toolkit::delegate_dispatch2!(@<S: Shell> Host<S>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_reach_egui_as_keys() {
        assert_eq!(egui_key(Keysym::p), Some(egui::Key::P));
        assert_eq!(egui_key(Keysym::N), Some(egui::Key::N));
        assert_eq!(egui_key(Keysym::_1), Some(egui::Key::Num1));
        assert_eq!(egui_key(Keysym::Escape), Some(egui::Key::Escape));
        assert_eq!(egui_key(Keysym::odiaeresis), None);
    }
}
