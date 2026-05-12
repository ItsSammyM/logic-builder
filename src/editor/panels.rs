use egui::{
    Button, Color32, Frame, Margin, RichText, SidePanel, Stroke, TextEdit, TextStyle,
    TopBottomPanel,
};

use super::app::App;
use super::constants::{COLOR_DIM, COLOR_PANEL_BG, COLOR_PORT_INPUT, COLOR_PORT_OUTPUT, COLOR_SIGNAL_HIGH};

impl App {
    // ─────────────────────────────────────────────────────────────────────────
    //  Top bar
    // ─────────────────────────────────────────────────────────────────────────

    pub fn show_top_panel(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("top_panel")
            .exact_height(52.0)
            .frame(
                Frame::none()
                    .fill(Color32::from_rgb(18, 20, 30))
                    .inner_margin(Margin::symmetric(12.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("⚡").size(22.0));
                    ui.add(
                        TextEdit::singleline(&mut self.title)
                            .desired_width(200.0)
                            .font(TextStyle::Heading),
                    );

                    ui.separator();

                    if ui
                        .button(RichText::new("💾  Save Gate").color(Color32::from_rgb(150, 210, 255)))
                        .on_hover_text("Save current circuit as a reusable gate in the library")
                        .clicked()
                    {
                        self.save_current_graph_to_library();
                    }

                    if ui.button("🗑  Clear").on_hover_text("Reset canvas").clicked() {
                        self.clear_canvas();
                    }

                    ui.separator();

                    let run_button_text = if self.simulation_running {
                        RichText::new("⏸  Pause").color(Color32::YELLOW)
                    } else {
                        RichText::new("▶  Run").color(COLOR_SIGNAL_HIGH)
                    };
                    if ui.button(run_button_text).clicked() {
                        if self.simulation_running {
                            self.simulation_running = false;
                        } else {
                            self.build_simulation_from_graph();
                            self.simulation_running = true;
                        }
                    }

                    if ui.button("⏭  Step").on_hover_text("Rebuild & advance one tick").clicked() {
                        self.simulation_running = false;
                        self.build_simulation_from_graph();
                        self.step_simulation();
                    }

                    ui.separator();

                    if let Some(error_message) = &self.simulation_error.clone() {
                        ui.label(
                            RichText::new(format!("⚠  {error_message}"))
                                .color(Color32::RED)
                                .size(11.5),
                        );
                    } else if self.simulation.is_some() {
                        ui.label(
                            RichText::new(if self.simulation_running { "● Running" } else { "● Built" })
                                .color(COLOR_SIGNAL_HIGH)
                                .size(12.0),
                        );
                    }
                });
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Left panel — inputs
    // ─────────────────────────────────────────────────────────────────────────

    pub fn show_left_panel(&mut self, ctx: &egui::Context) {
        SidePanel::left("left_panel")
            .resizable(false)
            .exact_width(158.0)
            .frame(
                Frame::none()
                    .fill(COLOR_PANEL_BG)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(55, 62, 90)))
                    .inner_margin(Margin::same(10.0)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("INPUTS").color(COLOR_PORT_INPUT).strong());
                ui.separator();

                let mut input_to_remove: Option<usize> = None;
                for input_index in 0..self.graph.inputs.len() {
                    ui.horizontal(|ui| {
                        let is_on = self.input_states.get(input_index).copied().unwrap_or(false);
                        if ui.small_button(if is_on { "🟢" } else { "⚫" }).clicked() {
                            if let Some(state) = self.input_states.get_mut(input_index) {
                                *state = !*state;
                            }
                            if self.simulation.is_some() {
                                self.step_simulation();
                            }
                        }
                        ui.add(
                            TextEdit::singleline(&mut self.graph.inputs[input_index])
                                .desired_width(70.0)
                                .font(TextStyle::Small),
                        );
                        if ui.small_button("✖").clicked() {
                            input_to_remove = Some(input_index);
                        }
                    });
                }
                if let Some(index) = input_to_remove {
                    self.graph.remove_input(index);
                    if index < self.input_states.len() {
                        self.input_states.remove(index);
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.new_input_name)
                            .desired_width(80.0)
                            .hint_text("name…"),
                    );
                    if ui.small_button("＋").clicked() {
                        let name = if self.new_input_name.is_empty() {
                            format!("I{}", self.graph.inputs.len())
                        } else {
                            std::mem::take(&mut self.new_input_name)
                        };
                        self.graph.inputs.push(name);
                        self.input_states.push(false);
                    }
                });

                ui.add_space(16.0);
                ui.label(RichText::new("TIPS").color(COLOR_DIM).strong().size(10.5));
                for hint_text in &[
                    "Right-click canvas\nto spawn gates",
                    "Click output (yellow)\nthen input (blue)\nto connect",
                    "Shift+drag on canvas\nto bulk-wire ports",
                    "Right-click a wire\nto delete it",
                    "Middle-drag/scroll\nto pan & zoom",
                    "Hover + Del\nto delete a gate",
                ] {
                    ui.label(RichText::new(*hint_text).color(COLOR_DIM).size(10.0));
                    ui.add_space(3.0);
                }
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Right panel — outputs & library
    // ─────────────────────────────────────────────────────────────────────────

    pub fn show_right_panel(&mut self, ctx: &egui::Context) {
        SidePanel::right("right_panel")
            .resizable(false)
            .exact_width(168.0)
            .frame(
                Frame::none()
                    .fill(COLOR_PANEL_BG)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(55, 62, 90)))
                    .inner_margin(Margin::same(10.0)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("OUTPUTS").color(COLOR_PORT_OUTPUT).strong());
                ui.separator();

                let mut output_to_remove: Option<usize> = None;
                for output_index in 0..self.graph.outputs.len() {
                    ui.horizontal(|ui| {
                        let is_on = self.output_states.get(output_index).copied().unwrap_or(false);
                        ui.label(if is_on { "🟡" } else { "⚫" });
                        ui.add(
                            TextEdit::singleline(&mut self.graph.outputs[output_index])
                                .desired_width(70.0)
                                .font(TextStyle::Small),
                        );
                        if ui.small_button("✖").clicked() {
                            output_to_remove = Some(output_index);
                        }
                    });
                }
                if let Some(index) = output_to_remove {
                    self.graph.remove_output(index);
                    if index < self.output_states.len() {
                        self.output_states.remove(index);
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(
                        TextEdit::singleline(&mut self.new_output_name)
                            .desired_width(80.0)
                            .hint_text("name…"),
                    );
                    if ui.small_button("＋").clicked() {
                        let name = if self.new_output_name.is_empty() {
                            format!("O{}", self.graph.outputs.len())
                        } else {
                            std::mem::take(&mut self.new_output_name)
                        };
                        self.graph.outputs.push(name);
                        self.output_states.push(false);
                    }
                });

                // ── Library ───────────────────────────────────────────────────

                ui.add_space(12.0);
                ui.label(RichText::new("LIBRARY").color(COLOR_DIM).strong().size(11.0));
                ui.separator();

                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Load").color(Color32::from_rgb(150, 210, 255)))
                        .on_hover_text("Load library from file")
                        .clicked()
                    {
                        self.load_library_from_file();
                    }
                    if !self.library.is_empty() {
                        if ui
                            .button(RichText::new("Save").color(Color32::from_rgb(150, 210, 255)))
                            .on_hover_text("Save library to file")
                            .clicked()
                        {
                            self.save_library_to_file();
                        }
                    }
                });

                if !self.library.is_empty() {
                    ui.label(
                        RichText::new("Right-click to manage")
                            .color(COLOR_DIM)
                            .italics()
                            .size(10.0),
                    );
                    ui.add_space(4.0);

                    // Collect actions deferred out of the borrow — we can't mutate
                    // self while iterating self.library.
                    let mut gate_to_open:   Option<usize> = None;
                    let mut gate_to_delete: Option<usize> = None;
                    let mut gate_to_rename: Option<usize> = None;
                    let mut commit_rename = false;

                    for (library_index, saved_gate) in self.library.iter().enumerate() {
                        let button_label = format!(
                            "▣ {}  ({} → {})",
                            saved_gate.name, saved_gate.input_count, saved_gate.output_count
                        );

                        let is_renaming = self.library_rename_index == Some(library_index);

                        if is_renaming {
                            ui.horizontal(|ui| {
                                ui.add(
                                    TextEdit::singleline(&mut self.library_rename_text)
                                        .desired_width(100.0)
                                        .font(TextStyle::Small),
                                );
                                if ui.small_button("✔").clicked() {
                                    commit_rename = true;
                                }
                            });
                            continue;
                        }

                        let button_response = ui.add(
                            Button::new(
                                RichText::new(&button_label)
                                    .size(11.0)
                                    .color(Color32::from_rgb(180, 200, 255)),
                            )
                            .frame(false),
                        );

                        button_response.context_menu(|ui| {
                            ui.label(RichText::new(&saved_gate.name).strong().size(12.0));
                            ui.separator();
                            if ui.button("📂  Open for editing").clicked() {
                                gate_to_open = Some(library_index);
                                ui.close_menu();
                            }
                            if ui.button("✏  Rename").clicked() {
                                gate_to_rename = Some(library_index);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button(RichText::new("🗑  Delete").color(Color32::RED)).clicked() {
                                gate_to_delete = Some(library_index);
                                ui.close_menu();
                            }
                        });
                    }

                    if commit_rename {
                        if let Some(rename_index) = self.library_rename_index {
                            let new_name = self.library_rename_text.trim().to_string();
                            if !new_name.is_empty() {
                                self.library[rename_index].name = new_name;
                            }
                        }
                        self.library_rename_index = None;
                        self.library_rename_text.clear();
                    }
                    if let Some(index) = gate_to_rename {
                        self.library_rename_text  = self.library[index].name.clone();
                        self.library_rename_index = Some(index);
                    }
                    if let Some(index) = gate_to_open {
                        self.open_library_gate_for_editing(index);
                    }
                    if let Some(index) = gate_to_delete {
                        self.delete_library_gate(index);
                    }
                }
            });
    }
}
