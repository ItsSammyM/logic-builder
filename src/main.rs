#![allow(unused, clippy::all)]

use egui::{Visuals, ViewportBuilder};

mod bit_array;
mod simulation;
mod sim_builder;
mod editor;

use editor::App;
use editor::constants::{COLOR_PANEL_BG, COLOR_TEXT};

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "Logic Gate Editor",
        eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_title("Logic Gate Editor")
                .with_inner_size([1440.0, 900.0])
                .with_min_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(|_cc| Box::new(App::default())),
    )
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut visuals = Visuals::dark();
        visuals.panel_fill = COLOR_PANEL_BG;
        visuals.override_text_color = Some(COLOR_TEXT);
        ctx.set_visuals(visuals);

        if self.simulation_running {
            self.step_simulation();
            ctx.request_repaint();
        }

        self.show_top_panel(ctx);
        self.show_left_panel(ctx);
        self.show_right_panel(ctx);
        self.show_canvas(ctx);
    }
}
