//! The open windows, from the compositor's wlr-foreign-toplevel-management,
//! for a shell piece that lists them (the bar's window tabs). A [`Shell`]
//! hears about changes through [`Shell::toplevels_changed`] and asks for
//! windows to be activated or closed through [`Shell::take_toplevel_requests`].
//!
//! [`Shell`]: crate::Shell
//! [`Shell::toplevels_changed`]: crate::Shell::toplevels_changed
//! [`Shell::take_toplevel_requests`]: crate::Shell::take_toplevel_requests

use std::sync::Arc;

use smithay_client_toolkit::{
    dispatch2::Dispatch2,
    reexports::{
        client::{Connection, QueueHandle, backend::ObjectData},
        protocols_wlr::foreign_toplevel::v1::client::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
    },
};

use crate::host::{Host, Shell};

/// An open window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toplevel {
    /// Stable for as long as the window is open.
    pub id: u64,
    pub title: String,
    pub app_id: String,
    /// Whether it's the window with the keyboard.
    pub activated: bool,
}

/// What a shell piece can ask of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToplevelRequest {
    Activate(u64),
    Close(u64),
}

/// The windows as the compositor last described them, each with the
/// changes it's still sending (they apply on its `done`).
#[derive(Default)]
pub(crate) struct Toplevels {
    pub(crate) manager: Option<ZwlrForeignToplevelManagerV1>,
    pub(crate) list: Vec<(ZwlrForeignToplevelHandleV1, Toplevel, Toplevel)>,
    next_id: u64,
}

impl Toplevels {
    pub(crate) fn current(&self) -> Vec<Toplevel> {
        self.list.iter().map(|(_, t, _)| t.clone()).collect()
    }

    pub(crate) fn handle(&self, id: u64) -> Option<&ZwlrForeignToplevelHandleV1> {
        self.list
            .iter()
            .find(|(_, t, _)| t.id == id)
            .map(|(h, _, _)| h)
    }
}

/// User data for the manager.
pub(crate) struct ManagerData;
/// User data for a window's handle.
pub(crate) struct HandleData;

impl<S: Shell> Dispatch2<ZwlrForeignToplevelManagerV1, Host<S>> for ManagerData {
    fn event(
        &self,
        host: &mut Host<S>,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<Host<S>>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                let t = &mut host.toplevels;
                t.next_id += 1;
                let fresh = Toplevel {
                    id: t.next_id,
                    ..Default::default()
                };
                t.list.push((toplevel, fresh.clone(), fresh));
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                host.toplevels = Toplevels::default();
                host.toplevels_changed();
            }
            _ => {}
        }
    }

    fn event_created_child(opcode: u16, qh: &QueueHandle<Host<S>>) -> Arc<dyn ObjectData> {
        if opcode == zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE {
            qh.make_data::<ZwlrForeignToplevelHandleV1, HandleData>(HandleData)
        } else {
            panic!("zwlr_foreign_toplevel_manager_v1 made an unknown object (opcode {opcode})")
        }
    }
}

impl<S: Shell> Dispatch2<ZwlrForeignToplevelHandleV1, Host<S>> for HandleData {
    fn event(
        &self,
        host: &mut Host<S>,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &Connection,
        _: &QueueHandle<Host<S>>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Event;
        let list = &mut host.toplevels.list;
        let Some(i) = list.iter().position(|(h, _, _)| h == handle) else {
            return;
        };
        let pending = &mut list[i].2;
        match event {
            Event::Title { title } => pending.title = title,
            Event::AppId { app_id } => pending.app_id = app_id,
            Event::State { state } => {
                let activated = zwlr_foreign_toplevel_handle_v1::State::Activated as u32;
                pending.activated = state
                    .chunks_exact(4)
                    .any(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) == activated);
            }
            Event::Done => {
                list[i].1 = list[i].2.clone();
                host.toplevels_changed();
            }
            Event::Closed => {
                let (handle, _, _) = list.remove(i);
                handle.destroy();
                host.toplevels_changed();
            }
            _ => {}
        }
    }
}

impl<S: Shell> Host<S> {
    fn toplevels_changed(&mut self) {
        let list = self.toplevels.current();
        self.shell.toplevels_changed(&list);
        self.redraw_all();
    }

    /// Passes the shell's activate and close requests to the compositor.
    pub(crate) fn send_toplevel_requests(&mut self) {
        let requests = self.shell.take_toplevel_requests();
        if requests.is_empty() {
            return;
        }
        let seat = self.seat_state.seats().next();
        for request in requests {
            match request {
                ToplevelRequest::Activate(id) => {
                    if let (Some(handle), Some(seat)) = (self.toplevels.handle(id), &seat) {
                        handle.activate(seat);
                    }
                }
                ToplevelRequest::Close(id) => {
                    if let Some(handle) = self.toplevels.handle(id) {
                        handle.close();
                    }
                }
            }
        }
    }
}
