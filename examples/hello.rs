//! A panel in the middle of the screen that says hello; Escape closes it.
//! On a layer-shell compositor (sway, Hyprland, Patina) it's an overlay; on
//! GNOME, a centred window: `cargo run --example hello`.

use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

struct Hello {
    done: bool,
}

impl Shell for Hello {
    fn setup(&mut self, ctx: &egui::Context) {
        patina::skin::Skin::default_skin().apply(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _output: &str) {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 12.0, ui.visuals().panel_fill);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(24.0)), |ui| {
            ui.heading("Hello from patina-shell");
            ui.label("Escape closes this.");
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.done = true;
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

fn main() -> Result<(), String> {
    let options = Options {
        namespace: "hello".into(),
        layer: Layer::Overlay,
        // No anchor centres it.
        anchor: Anchor::empty(),
        size: (360, 140),
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: true,
    };
    let (host, event_loop) = Host::connect(options, Hello { done: false })?;
    host.run(event_loop)
}
