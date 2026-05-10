#![allow(unused, clippy::all)]

use egui::*;
use std::collections::HashMap;

mod bit_array;
mod simulation;
mod sim_builder;

use serde::{Deserialize, Serialize, ser::SerializeStruct as _};
use sim_builder::{build_simulation, read_wire_by_index, GateKind, GraphDesc, PortRef, WireDesc};
use simulation::simulation::Simulation;

// ─────────────────────────────────────────────────────────────────────────────
//  Editor data model
// ─────────────────────────────────────────────────────────────────────────────

/// One node placed on the canvas.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct EditorNode {
    label: String,
    /// Top-left corner in canvas space (not screen space).
    #[serde(with = "pos2_serde")]
    pos: Pos2,
    input_count: usize,
    output_count: usize,
    kind: EditorNodeKind,
}

mod pos2_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(pos: &Pos2, s: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        (pos.x, pos.y).serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Pos2, D::Error>
    where D: Deserializer<'de> {
        let (x, y) = <(f32, f32)>::deserialize(d)?;
        Ok(Pos2 { x, y })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum EditorNodeKind {
    Nand,
    /// Index into App::library, identifying which saved gate this instance represents.
    SavedGate(usize),
}

/// A connection from one node's output port to another node's input port.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Wire {
    from: PortRef,
    to: PortRef,
}

/// A gate that has been saved to the library.
/// Stores both display metadata and the complete editor graph so it can be re-opened.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct LibraryGate {
    name: String,
    input_count: usize,
    output_count: usize,
    graph: EditorGraph,
}

/// The editor's full representation of one circuit level.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct EditorGraph {
    /// Names of the external input ports shown on the left rail.
    inputs: Vec<String>,
    /// Names of the external output ports shown on the right rail.
    outputs: Vec<String>,
    /// Internal gate nodes placed on the canvas.
    nodes: Vec<EditorNode>,
    /// All wires connecting ports to each other.
    wires: Vec<Wire>,
}

impl Default for EditorGraph {
    fn default() -> Self {
        Self {
            inputs: vec!["I0".into()],
            outputs: vec!["O0".into()],
            nodes: vec![],
            wires: vec![],
        }
    }
}

impl EditorGraph {
    // Node-id layout (flat integer used in PortRef::node):
    //   0 .. inputs.len()-1                              → input pseudo-nodes
    //   inputs.len() .. inputs.len()+outputs.len()-1     → output pseudo-nodes
    //   inputs.len()+outputs.len() ..                    → internal gate nodes

    fn input_node_id(&self, input_index: usize) -> usize {
        input_index
    }
    fn output_node_id(&self, output_index: usize) -> usize {
        self.inputs.len() + output_index
    }
    fn gate_node_id(&self, gate_index: usize) -> usize {
        self.inputs.len() + self.outputs.len() + gate_index
    }

    fn is_input_node(&self, node_id: usize) -> bool {
        node_id < self.inputs.len()
    }
    fn is_output_node(&self, node_id: usize) -> bool {
        node_id >= self.inputs.len() && node_id < self.inputs.len() + self.outputs.len()
    }
    /// Converts a flat node_id back to an index into `self.nodes`, if it refers to a gate.
    fn gate_index_from_node_id(&self, node_id: usize) -> Option<usize> {
        let gate_base = self.inputs.len() + self.outputs.len();
        if node_id >= gate_base && node_id - gate_base < self.nodes.len() {
            Some(node_id - gate_base)
        } else {
            None
        }
    }

    // ── Structural mutations that keep wires consistent ───────────────────────
    //
    // The root cause of the deletion bugs is that PortRef::node is a *flat* id
    // (not a stable handle), so removing any node shifts the ids of everything
    // that follows it.  Every removal must rewrite all wire node-ids accordingly.

    /// Remove an input at position `input_index` and fix up all wire node-ids.
    fn remove_input(&mut self, input_index: usize) {
        let removed_node_id = self.input_node_id(input_index);

        // Drop wires that originate from the removed input.
        self.wires.retain(|wire| wire.from.node != removed_node_id);

        // Any wire referencing a node-id > removed_node_id must be decremented by 1,
        // because removing one input shifts every subsequent input, every output, and
        // every gate down by one in the flat id space.
        for wire in &mut self.wires {
            if wire.from.node > removed_node_id { wire.from.node -= 1; }
            if wire.to.node   > removed_node_id { wire.to.node   -= 1; }
        }

        self.inputs.remove(input_index);
    }

    /// Remove an output at position `output_index` and fix up all wire node-ids.
    fn remove_output(&mut self, output_index: usize) {
        let removed_node_id = self.output_node_id(output_index);

        // Drop wires that feed the removed output.
        self.wires.retain(|wire| wire.to.node != removed_node_id);

        for wire in &mut self.wires {
            if wire.from.node > removed_node_id { wire.from.node -= 1; }
            if wire.to.node   > removed_node_id { wire.to.node   -= 1; }
        }

        self.outputs.remove(output_index);
    }

    /// Remove a gate at position `gate_index` and fix up all wire node-ids.
    fn remove_gate(&mut self, gate_index: usize) {
        let removed_node_id = self.gate_node_id(gate_index);

        // Drop wires connected to the removed gate.
        self.wires.retain(|wire| {
            wire.from.node != removed_node_id && wire.to.node != removed_node_id
        });

        for wire in &mut self.wires {
            if wire.from.node > removed_node_id { wire.from.node -= 1; }
            if wire.to.node   > removed_node_id { wire.to.node   -= 1; }
        }

        self.nodes.remove(gate_index);
    }

    /// Remove a specific wire (by value equality).
    fn remove_wire(&mut self, wire_to_remove: &Wire) {
        self.wires.retain(|wire| wire != wire_to_remove);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Multi-port selection state (for bulk wiring)
// ─────────────────────────────────────────────────────────────────────────────

/// Which phase of the box-select bulk-wire operation we are in.
#[derive(Clone, Debug, Default)]
enum BulkWireState {
    /// Nothing happening.
    #[default]
    Idle,
    /// User is dragging to select output ports.
    SelectingOutputs {
        drag_start_canvas: Pos2,
        drag_current_canvas: Pos2,
    },
    /// Outputs selected, waiting for user to box-select inputs.
    OutputsChosen {
        /// The output PortRefs collected in phase 1, in top-to-bottom order.
        selected_output_ports: Vec<PortRef>,
    },
    /// User is dragging to select input ports (phase 2).
    SelectingInputs {
        selected_output_ports: Vec<PortRef>,
        drag_start_canvas: Pos2,
        drag_current_canvas: Pos2,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
//  App
// ─────────────────────────────────────────────────────────────────────────────

struct App {
    title: String,
    graph: EditorGraph,
    library: Vec<LibraryGate>,

    simulation: Option<Simulation>,
    /// wire_index → current signal value, rebuilt each tick by querying the simulation.
    live_wire_signals: HashMap<u32, bool>,
    /// Maps (node_id, port_index, is_output) → wire_index in the simulation.
    /// Built alongside the simulation so draw code can look up signal state.
    port_to_wire_index: HashMap<(usize, usize, bool), u32>,
    input_states: Vec<bool>,
    output_states: Vec<bool>,
    simulation_running: bool,
    simulation_error: Option<String>,

    canvas_pan: Vec2,
    canvas_zoom: f32,

    /// The output port the user clicked to start drawing a single wire from.
    pending_wire_start: Option<PortRef>,
    /// Index of the gate being dragged, plus the offset from its top-left corner to the click point.
    dragging_gate: Option<(usize, Vec2)>,

    /// State machine for the box-select bulk-wire feature (Shift+drag).
    bulk_wire_state: BulkWireState,

    new_input_name: String,
    new_output_name: String,

    /// Library gate currently showing its right-click popup (for rename).
    library_rename_index: Option<usize>,
    library_rename_text: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            title: "My Gate".into(),
            graph: EditorGraph::default(),
            library: vec![],
            simulation: None,
            live_wire_signals: HashMap::new(),
            port_to_wire_index: HashMap::new(),
            input_states: vec![false],
            output_states: vec![false],
            simulation_running: false,
            simulation_error: None,
            canvas_pan: Vec2::ZERO,
            canvas_zoom: 1.0,
            pending_wire_start: None,
            dragging_gate: None,
            bulk_wire_state: BulkWireState::Idle,
            new_input_name: String::new(),
            new_output_name: String::new(),
            library_rename_index: None,
            library_rename_text: String::new(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Layout constants
// ─────────────────────────────────────────────────────────────────────────────

const NODE_WIDTH: f32 = 108.0;
/// Vertical distance from the top of a node body to its first port dot.
const PORT_TOP_PADDING: f32 = 28.0;
/// Vertical distance between consecutive port dots on the same node.
const PORT_VERTICAL_STEP: f32 = 22.0;
const PORT_RADIUS: f32 = 6.0;
const GRID_CELL_SIZE: f32 = 20.0;
/// Vertical distance between consecutive I/O rail port dots.
const IO_RAIL_STEP: f32 = 52.0;

const COLOR_BACKGROUND: Color32    = Color32::from_rgb(22, 24, 34);
const COLOR_GRID: Color32          = Color32::from_rgb(38, 42, 58);
const COLOR_PANEL_BG: Color32      = Color32::from_rgb(28, 30, 42);
const COLOR_NODE_FILL: Color32     = Color32::from_rgb(48, 52, 76);
const COLOR_NODE_HOVERED: Color32  = Color32::from_rgb(68, 74, 110);
const COLOR_NODE_STROKE: Color32   = Color32::from_rgb(90, 100, 150);
const COLOR_PORT_INPUT: Color32    = Color32::from_rgb(80, 175, 235);
const COLOR_PORT_OUTPUT: Color32   = Color32::from_rgb(235, 175, 60);
const COLOR_WIRE: Color32          = Color32::from_rgb(80, 100, 110);
const COLOR_WIRE_HIGH: Color32     = Color32::from_rgb(100, 230, 120);
const COLOR_WIRE_LOW: Color32      = Color32::from_rgb(60, 80, 100);
const COLOR_WIRE_PENDING: Color32  = Color32::from_rgb(240, 220, 60);
const COLOR_SIGNAL_HIGH: Color32   = Color32::from_rgb(70, 230, 90);
const COLOR_SIGNAL_LOW: Color32    = Color32::from_rgb(55, 60, 85);
const COLOR_TEXT: Color32          = Color32::from_rgb(210, 218, 255);
const COLOR_DIM: Color32           = Color32::from_rgb(120, 130, 170);
const COLOR_BOX_SELECT: Color32    = Color32::from_rgba_premultiplied(80, 160, 255, 30);
const COLOR_BOX_SELECT_BORDER: Color32 = Color32::from_rgb(80, 160, 255);

// ─────────────────────────────────────────────────────────────────────────────
//  main
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
//  eframe::App
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
//  Panel implementations
// ─────────────────────────────────────────────────────────────────────────────

impl App {
    // ── top bar ───────────────────────────────────────────────────────────────

    fn show_top_panel(&mut self, ctx: &egui::Context) {
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

    // ── left panel — inputs ───────────────────────────────────────────────────

    fn show_left_panel(&mut self, ctx: &egui::Context) {
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
                            if self.simulation.is_some() { self.step_simulation(); }
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
                    if index < self.input_states.len() { self.input_states.remove(index); }
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

    // ── right panel — outputs & library ──────────────────────────────────────

    fn show_right_panel(&mut self, ctx: &egui::Context) {
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
                    if index < self.output_states.len() { self.output_states.remove(index); }
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

                // ── library ───────────────────────────────────────────────

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
                    let mut gate_to_open: Option<usize>   = None;
                    let mut gate_to_delete: Option<usize> = None;
                    let mut gate_to_rename: Option<usize> = None;

                    // Collect rename commits separately (needs two indices at once).
                    let mut commit_rename = false;

                    for (library_index, saved_gate) in self.library.iter().enumerate() {
                        let button_label = format!(
                            "▣ {}  ({} → {})",
                            saved_gate.name, saved_gate.input_count, saved_gate.output_count
                        );

                        let is_renaming = self.library_rename_index == Some(library_index);

                        let button_response = if is_renaming {
                            // Show rename text field inline instead of the button.
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
                            // Return a dummy response we won't use — the rename row
                            // doesn't need a context menu.
                            continue;
                        } else {
                            ui.add(
                                Button::new(
                                    RichText::new(&button_label)
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 200, 255)),
                                )
                                .frame(false),
                            )
                        };

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

                    // Commit a rename if the user clicked ✔.
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
                        self.library_rename_text = self.library[index].name.clone();
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

    // ── canvas ────────────────────────────────────────────────────────────────

    fn show_canvas(&mut self, ctx: &egui::Context) {
        CentralPanel::default()
            .frame(Frame::none().fill(COLOR_BACKGROUND))
            .show(ctx, |ui| {
                let (canvas_response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let canvas_origin = canvas_response.rect.min;
                let canvas_rect   = canvas_response.rect;

                // ── zoom ──────────────────────────────────────────────────
                let scroll_delta = ui.input(|input| input.smooth_scroll_delta.y);
                if scroll_delta != 0.0 && canvas_response.hovered() {
                    self.canvas_zoom =
                        (self.canvas_zoom * (1.0 + scroll_delta * 0.0012)).clamp(0.25, 4.0);
                }

                // ── pan (middle mouse) ────────────────────────────────────
                if canvas_response.dragged_by(PointerButton::Middle) {
                    self.canvas_pan += canvas_response.drag_delta() / self.canvas_zoom;
                }

                let pointer_screen_pos = ui.input(|input| input.pointer.interact_pos());
                let shift_held = ui.input(|input| input.modifiers.shift);

                let hovered_port =
                    pointer_screen_pos.and_then(|pos| self.hit_test_port(pos, canvas_origin, canvas_rect));
                let hovered_gate_index =
                    pointer_screen_pos.and_then(|pos| self.hit_test_gate(pos, canvas_origin));
                let hovered_wire =
                    pointer_screen_pos.and_then(|pos| self.hit_test_wire(pos, canvas_origin, canvas_rect));

                // ── end gate drag ─────────────────────────────────────────
                if canvas_response.drag_stopped() {
                    self.dragging_gate = None;
                }

                // ── box-select bulk-wire (Shift+drag) ─────────────────────
                self.update_bulk_wire(
                    &canvas_response,
                    pointer_screen_pos,
                    shift_held,
                    canvas_origin,
                    canvas_rect,
                );

                // ── continue gate drag ────────────────────────────────────
                if matches!(self.bulk_wire_state, BulkWireState::Idle) {
                    if let Some((dragged_gate_index, drag_offset)) = self.dragging_gate {
                        if canvas_response.dragged_by(PointerButton::Primary) {
                            if let Some(pointer_pos) = pointer_screen_pos {
                                let raw_canvas_pos =
                                    self.screen_to_canvas(pointer_pos, canvas_origin) + drag_offset;
                                self.graph.nodes[dragged_gate_index].pos = Pos2::new(
                                    (raw_canvas_pos.x / GRID_CELL_SIZE).round() * GRID_CELL_SIZE,
                                    (raw_canvas_pos.y / GRID_CELL_SIZE).round() * GRID_CELL_SIZE,
                                );
                            }
                        }
                    }
                    // ── start gate drag ───────────────────────────────────
                    else if canvas_response.drag_started_by(PointerButton::Primary)
                        && hovered_port.is_none()
                        && !shift_held
                    {
                        if let (Some(gate_index), Some(pointer_pos)) =
                            (hovered_gate_index, pointer_screen_pos)
                        {
                            let drag_offset = self.graph.nodes[gate_index].pos
                                - self.screen_to_canvas(pointer_pos, canvas_origin);
                            self.dragging_gate = Some((gate_index, drag_offset));
                        }
                    }
                }

                // ── port click → single wire ──────────────────────────────
                if canvas_response.clicked() && !shift_held {
                    if let Some((clicked_port, is_output_port)) = hovered_port.clone() {
                        match self.pending_wire_start.take() {
                            None => {
                                if is_output_port {
                                    self.pending_wire_start = Some(clicked_port);
                                }
                            }
                            Some(wire_start) => {
                                if !is_output_port {
                                    self.graph.wires.retain(|wire| {
                                        !(wire.to.node == clicked_port.node
                                            && wire.to.port == clicked_port.port)
                                    });
                                    self.graph.wires.push(Wire { from: wire_start, to: clicked_port });
                                } else {
                                    self.pending_wire_start = Some(clicked_port);
                                }
                            }
                        }
                    } else {
                        self.pending_wire_start = None;
                    }
                }

                // ── right-click wire → delete it ──────────────────────────
                if canvas_response.secondary_clicked() {
                    if let Some(wire) = hovered_wire.clone() {
                        if hovered_port.is_none() && hovered_gate_index.is_none() {
                            self.graph.remove_wire(&wire);
                        }
                    }
                }

                // ── delete hovered gate (Del/Backspace) ───────────────────
                if ui.input(|input| input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace)) {
                    if let Some(gate_index) = hovered_gate_index {
                        self.graph.remove_gate(gate_index);
                    }
                }

                // ── right-click context menu ──────────────────────────────
                let spawn_canvas_pos = pointer_screen_pos
                    .map(|pos| snap_to_grid(self.screen_to_canvas(pos, canvas_origin)))
                    .unwrap_or(Pos2::ZERO);

                // Only show the spawn menu if we're not hovering a wire or gate.
                if hovered_wire.is_none() && hovered_gate_index.is_none() {
                    canvas_response.context_menu(|ui| {
                        ui.label(RichText::new("Add Gate").strong().size(13.0));
                        ui.separator();

                        if ui.button("⊼  NAND  (2→1)").clicked() {
                            self.graph.nodes.push(make_nand_node(spawn_canvas_pos));
                            ui.close_menu();
                        }

                        if !self.library.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("Library").color(COLOR_DIM).italics().size(11.0));
                            let mut gate_to_spawn: Option<usize> = None;
                            for (library_index, saved_gate) in self.library.iter().enumerate() {
                                let button_label = format!(
                                    "▣  {}  ({} → {})",
                                    saved_gate.name, saved_gate.input_count, saved_gate.output_count
                                );
                                if ui.button(button_label).clicked() {
                                    gate_to_spawn = Some(library_index);
                                    ui.close_menu();
                                }
                            }
                            if let Some(library_index) = gate_to_spawn {
                                let saved_gate = &self.library[library_index];
                                self.graph.nodes.push(EditorNode {
                                    label: saved_gate.name.clone(),
                                    pos: spawn_canvas_pos,
                                    input_count: saved_gate.input_count,
                                    output_count: saved_gate.output_count,
                                    kind: EditorNodeKind::SavedGate(library_index),
                                });
                            }
                        }
                    });
                }

                // ─ draw ──────────────────────────────────────────────────
                self.draw_grid(&painter, canvas_rect);
                self.draw_io_rails(&painter, canvas_rect);
                self.draw_wires(&painter, canvas_origin, canvas_rect, &hovered_wire);

                // In-progress wire preview following cursor.
                if let Some(wire_start_port) = &self.pending_wire_start.clone() {
                    let start_screen =
                        self.port_to_screen_pos(wire_start_port, true, canvas_origin, canvas_rect);
                    if let (Some(start_pos), Some(mouse_pos)) = (start_screen, pointer_screen_pos) {
                        draw_bezier_wire(&painter, start_pos, mouse_pos, COLOR_WIRE_PENDING, 2.0);
                    }
                }

                // Gate nodes.
                for gate_index in 0..self.graph.nodes.len() {
                    let is_hovered = hovered_gate_index == Some(gate_index)
                        && self.dragging_gate.map(|(dragged, _)| dragged) != Some(gate_index);
                    let is_dragging =
                        self.dragging_gate.map(|(dragged, _)| dragged) == Some(gate_index);
                    self.draw_gate_node(gate_index, &painter, canvas_origin, canvas_rect, is_hovered, is_dragging);
                }

                // Highlight ring on port under mouse.
                if let Some((hovered_port_ref, is_output_port)) = &hovered_port {
                    if let Some(pos) = self.port_to_screen_pos(
                        hovered_port_ref, *is_output_port, canvas_origin, canvas_rect,
                    ) {
                        let highlight_color =
                            if *is_output_port { COLOR_PORT_OUTPUT } else { COLOR_PORT_INPUT };
                        painter.circle_stroke(
                            pos,
                            PORT_RADIUS * self.canvas_zoom * 2.0,
                            Stroke::new(2.0, highlight_color),
                        );
                    }
                }

                // Bulk-wire box-select overlays.
                self.draw_bulk_wire_overlay(&painter, canvas_origin, canvas_rect);
            });
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Bulk wire (Shift+drag box-select)
    // ─────────────────────────────────────────────────────────────────────────

    /// Drive the bulk-wire state machine from canvas input.
    fn update_bulk_wire(
        &mut self,
        canvas_response: &Response,
        pointer_screen_pos: Option<Pos2>,
        shift_held: bool,
        canvas_origin: Pos2,
        canvas_rect: Rect,
    ) {
        let pointer_canvas_pos = pointer_screen_pos
            .map(|pos| self.screen_to_canvas(pos, canvas_origin));

        match std::mem::take(&mut self.bulk_wire_state) {

            // ── Idle: start phase 1 if Shift+drag begins ──────────────────
            BulkWireState::Idle => {
                if shift_held && canvas_response.drag_started_by(PointerButton::Primary) {
                    if let Some(start) = pointer_canvas_pos {
                        self.bulk_wire_state = BulkWireState::SelectingOutputs {
                            drag_start_canvas:   start,
                            drag_current_canvas: start,
                        };
                    } else {
                        self.bulk_wire_state = BulkWireState::Idle;
                    }
                } else {
                    self.bulk_wire_state = BulkWireState::Idle;
                }
            }

            // ── Phase 1: user is dragging to select outputs ───────────────
            BulkWireState::SelectingOutputs { drag_start_canvas, .. } => {
                if canvas_response.dragged_by(PointerButton::Primary) {
                    // Update the current drag corner.
                    let current = pointer_canvas_pos.unwrap_or(drag_start_canvas);
                    self.bulk_wire_state = BulkWireState::SelectingOutputs {
                        drag_start_canvas,
                        drag_current_canvas: current,
                    };
                } else if canvas_response.drag_stopped() {
                    // Phase 1 finished — collect output ports inside the box.
                    let current = pointer_canvas_pos.unwrap_or(drag_start_canvas);
                    let selection_rect = canvas_rect_from_two_points(drag_start_canvas, current);
                    let mut selected_output_ports =
                        self.collect_ports_in_canvas_rect(selection_rect, true, canvas_rect);
                    // Sort top-to-bottom by their screen y position.
                    selected_output_ports.sort_by(|a_port, b_port| {
                        let a_y = self
                            .port_to_screen_pos(a_port, true, canvas_origin, canvas_rect)
                            .map(|p| p.y)
                            .unwrap_or(0.0);
                        let b_y = self
                            .port_to_screen_pos(b_port, true, canvas_origin, canvas_rect)
                            .map(|p| p.y)
                            .unwrap_or(0.0);
                        a_y.partial_cmp(&b_y).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    if selected_output_ports.is_empty() {
                        self.bulk_wire_state = BulkWireState::Idle;
                    } else {
                        self.bulk_wire_state = BulkWireState::OutputsChosen { selected_output_ports };
                    }
                } else {
                    // Drag cancelled or modifier released.
                    self.bulk_wire_state = BulkWireState::Idle;
                }
            }

            // ── Waiting: outputs chosen, now Shift+drag to select inputs ──
            BulkWireState::OutputsChosen { selected_output_ports } => {
                if shift_held && canvas_response.drag_started_by(PointerButton::Primary) {
                    if let Some(start) = pointer_canvas_pos {
                        self.bulk_wire_state = BulkWireState::SelectingInputs {
                            selected_output_ports,
                            drag_start_canvas: start,
                            drag_current_canvas: start,
                        };
                    } else {
                        self.bulk_wire_state = BulkWireState::OutputsChosen { selected_output_ports };
                    }
                } else if !shift_held && canvas_response.clicked() {
                    // User clicked without Shift — cancel.
                    self.bulk_wire_state = BulkWireState::Idle;
                } else {
                    self.bulk_wire_state = BulkWireState::OutputsChosen { selected_output_ports };
                }
            }

            // ── Phase 2: user is dragging to select inputs ────────────────
            BulkWireState::SelectingInputs {
                selected_output_ports,
                drag_start_canvas,
                ..
            } => {
                if canvas_response.dragged_by(PointerButton::Primary) {
                    let current = pointer_canvas_pos.unwrap_or(drag_start_canvas);
                    self.bulk_wire_state = BulkWireState::SelectingInputs {
                        selected_output_ports,
                        drag_start_canvas,
                        drag_current_canvas: current,
                    };
                } else if canvas_response.drag_stopped() {
                    // Phase 2 finished — collect input ports and wire them up.
                    let current = pointer_canvas_pos.unwrap_or(drag_start_canvas);
                    let selection_rect = canvas_rect_from_two_points(drag_start_canvas, current);
                    let mut selected_input_ports =
                        self.collect_ports_in_canvas_rect(selection_rect, false, canvas_rect);
                    selected_input_ports.sort_by(|a_port, b_port| {
                        let a_y = self
                            .port_to_screen_pos(a_port, false, canvas_origin, canvas_rect)
                            .map(|p| p.y)
                            .unwrap_or(0.0);
                        let b_y = self
                            .port_to_screen_pos(b_port, false, canvas_origin, canvas_rect)
                            .map(|p| p.y)
                            .unwrap_or(0.0);
                        a_y.partial_cmp(&b_y).unwrap_or(std::cmp::Ordering::Equal)
                    });

                    // Pair outputs to inputs in order (shortest list decides count).
                    let pair_count = selected_output_ports.len().min(selected_input_ports.len());
                    for pair_index in 0..pair_count {
                        let output_port = selected_output_ports[pair_index].clone();
                        let input_port  = selected_input_ports[pair_index].clone();
                        // Remove any existing wire driving this input port first.
                        self.graph.wires.retain(|wire| {
                            !(wire.to.node == input_port.node && wire.to.port == input_port.port)
                        });
                        self.graph.wires.push(Wire { from: output_port, to: input_port });
                    }

                    self.bulk_wire_state = BulkWireState::Idle;
                } else {
                    self.bulk_wire_state = BulkWireState::Idle;
                }
            }
        }
    }

    /// Return all ports of the given kind (output or input) whose screen position
    /// falls inside `canvas_rect_selection` (in canvas space).
    fn collect_ports_in_canvas_rect(
        &self,
        canvas_rect_selection: Rect,
        want_output_ports: bool,
        full_canvas_rect: Rect,
    ) -> Vec<PortRef> {
        let mut found_ports: Vec<PortRef> = Vec::new();
        let dummy_origin = full_canvas_rect.min; // not used for canvas-space comparison

        // Check I/O rail ports.
        let input_count  = self.graph.inputs.len();
        let output_count = self.graph.outputs.len();
        let center_y = full_canvas_rect.center().y;

        if want_output_ports {
            let start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            for input_index in 0..input_count {
                let screen_pos = pos2(
                    full_canvas_rect.left() + 14.0,
                    start_y + input_index as f32 * IO_RAIL_STEP,
                );
                let canvas_pos = self.screen_to_canvas(screen_pos, dummy_origin);
                if canvas_rect_selection.contains(canvas_pos) {
                    found_ports.push(PortRef { node: self.graph.input_node_id(input_index), port: 0 });
                }
            }
        } else {
            let start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            for output_index in 0..output_count {
                let screen_pos = pos2(
                    full_canvas_rect.right() - 14.0,
                    start_y + output_index as f32 * IO_RAIL_STEP,
                );
                let canvas_pos = self.screen_to_canvas(screen_pos, dummy_origin);
                if canvas_rect_selection.contains(canvas_pos) {
                    found_ports.push(PortRef { node: self.graph.output_node_id(output_index), port: 0 });
                }
            }
        }

        // Check gate node ports.
        for gate_index in 0..self.graph.nodes.len() {
            let node = &self.graph.nodes[gate_index];
            if want_output_ports {
                for port_index in 0..node.output_count {
                    let canvas_pos = output_port_canvas_pos(node, port_index);
                    if canvas_rect_selection.contains(canvas_pos) {
                        found_ports.push(PortRef {
                            node: self.graph.gate_node_id(gate_index),
                            port: port_index,
                        });
                    }
                }
            } else {
                for port_index in 0..node.input_count {
                    let canvas_pos = input_port_canvas_pos(node, port_index);
                    if canvas_rect_selection.contains(canvas_pos) {
                        found_ports.push(PortRef {
                            node: self.graph.gate_node_id(gate_index),
                            port: port_index,
                        });
                    }
                }
            }
        }

        found_ports
    }

    /// Draw the box-select rectangles and selected-port highlights.
    fn draw_bulk_wire_overlay(&self, painter: &Painter, canvas_origin: Pos2, canvas_rect: Rect) {
        match &self.bulk_wire_state {
            BulkWireState::SelectingOutputs { drag_start_canvas, drag_current_canvas } => {
                let start_screen = self.canvas_to_screen(*drag_start_canvas, canvas_origin);
                let end_screen   = self.canvas_to_screen(*drag_current_canvas, canvas_origin);
                let screen_rect  = Rect::from_two_pos(start_screen, end_screen);
                painter.rect(
                    screen_rect,
                    Rounding::ZERO,
                    COLOR_BOX_SELECT,
                    Stroke::new(1.5, COLOR_BOX_SELECT_BORDER),
                );
                painter.text(
                    screen_rect.left_top() - vec2(0.0, 14.0),
                    Align2::LEFT_BOTTOM,
                    "Select OUTPUT ports",
                    FontId::proportional(11.0),
                    COLOR_BOX_SELECT_BORDER,
                );
            }

            BulkWireState::OutputsChosen { selected_output_ports } => {
                // Highlight chosen output ports with green rings.
                for port in selected_output_ports {
                    if let Some(screen_pos) =
                        self.port_to_screen_pos(port, true, canvas_origin, canvas_rect)
                    {
                        painter.circle_stroke(
                            screen_pos,
                            PORT_RADIUS * self.canvas_zoom * 2.5,
                            Stroke::new(2.5, COLOR_SIGNAL_HIGH),
                        );
                    }
                }
                // Label instructing the user.
                painter.text(
                    canvas_rect.center_top() + vec2(0.0, 8.0),
                    Align2::CENTER_TOP,
                    format!("{} outputs selected — Shift+drag to pick inputs", selected_output_ports.len()),
                    FontId::proportional(12.0),
                    COLOR_SIGNAL_HIGH,
                );
            }

            BulkWireState::SelectingInputs {
                selected_output_ports,
                drag_start_canvas,
                drag_current_canvas,
            } => {
                // Show selected outputs.
                for port in selected_output_ports {
                    if let Some(screen_pos) =
                        self.port_to_screen_pos(port, true, canvas_origin, canvas_rect)
                    {
                        painter.circle_stroke(
                            screen_pos,
                            PORT_RADIUS * self.canvas_zoom * 2.5,
                            Stroke::new(2.5, COLOR_SIGNAL_HIGH),
                        );
                    }
                }
                // Show input selection box.
                let start_screen = self.canvas_to_screen(*drag_start_canvas, canvas_origin);
                let end_screen   = self.canvas_to_screen(*drag_current_canvas, canvas_origin);
                let screen_rect  = Rect::from_two_pos(start_screen, end_screen);
                painter.rect(
                    screen_rect,
                    Rounding::ZERO,
                    Color32::from_rgba_premultiplied(255, 120, 80, 25),
                    Stroke::new(1.5, Color32::from_rgb(255, 120, 80)),
                );
                painter.text(
                    screen_rect.left_top() - vec2(0.0, 14.0),
                    Align2::LEFT_BOTTOM,
                    "Select INPUT ports",
                    FontId::proportional(11.0),
                    Color32::from_rgb(255, 140, 80),
                );
            }

            BulkWireState::Idle => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Drawing helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn draw_grid(&self, painter: &Painter, rect: Rect) {
        let grid_pixel_size = GRID_CELL_SIZE * self.canvas_zoom;
        let pan_remainder_x = (self.canvas_pan.x * self.canvas_zoom).rem_euclid(grid_pixel_size);
        let pan_remainder_y = (self.canvas_pan.y * self.canvas_zoom).rem_euclid(grid_pixel_size);
        let stroke = Stroke::new(0.5, COLOR_GRID);

        let mut x = rect.left() + pan_remainder_x;
        while x <= rect.right() {
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
            x += grid_pixel_size;
        }
        let mut y = rect.top() + pan_remainder_y;
        while y <= rect.bottom() {
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], stroke);
            y += grid_pixel_size;
        }
    }

    fn draw_io_rails(&self, painter: &Painter, rect: Rect) {
        let center_y = rect.center().y;
        let input_count  = self.graph.inputs.len();
        let output_count = self.graph.outputs.len();

        let inputs_start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for (input_index, input_name) in self.graph.inputs.iter().enumerate() {
            let screen_pos = pos2(rect.left() + 14.0, inputs_start_y + input_index as f32 * IO_RAIL_STEP);
            let is_on = self.input_states.get(input_index).copied().unwrap_or(false);
            let signal_color = if is_on { COLOR_SIGNAL_HIGH } else { COLOR_SIGNAL_LOW };
            painter.circle_filled(screen_pos, PORT_RADIUS + 3.0, signal_color);
            painter.circle_stroke(screen_pos, PORT_RADIUS + 3.0, Stroke::new(1.5, COLOR_PORT_INPUT));
            painter.text(screen_pos + vec2(14.0, 0.0), Align2::LEFT_CENTER, input_name,
                         FontId::proportional(12.0), COLOR_TEXT);
        }

        let outputs_start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for (output_index, output_name) in self.graph.outputs.iter().enumerate() {
            let screen_pos = pos2(rect.right() - 14.0, outputs_start_y + output_index as f32 * IO_RAIL_STEP);
            let is_on = self.output_states.get(output_index).copied().unwrap_or(false);
            let signal_color = if is_on { COLOR_SIGNAL_HIGH } else { COLOR_SIGNAL_LOW };
            painter.circle_filled(screen_pos, PORT_RADIUS + 3.0, signal_color);
            painter.circle_stroke(screen_pos, PORT_RADIUS + 3.0, Stroke::new(1.5, COLOR_PORT_OUTPUT));
            painter.text(screen_pos - vec2(14.0, 0.0), Align2::RIGHT_CENTER, output_name,
                         FontId::proportional(12.0), COLOR_TEXT);
        }
    }

    fn draw_wires(
        &self,
        painter: &Painter,
        canvas_origin: Pos2,
        canvas_rect: Rect,
        hovered_wire: &Option<Wire>,
    ) {
        for wire in &self.graph.wires {
            let from_screen = self.port_to_screen_pos(&wire.from, true,  canvas_origin, canvas_rect);
            let to_screen   = self.port_to_screen_pos(&wire.to,   false, canvas_origin, canvas_rect);
            if let (Some(from_pos), Some(to_pos)) = (from_screen, to_screen) {
                // Determine signal color from the driving output port's wire index.
                let wire_color = if self.simulation.is_some() {
                    let wire_index = self.port_to_wire_index
                        .get(&(wire.from.node, wire.from.port, true))
                        .copied();
                    match wire_index {
                        Some(idx) if self.live_wire_signals.get(&idx).copied().unwrap_or(false) => {
                            COLOR_WIRE_HIGH
                        }
                        Some(_) => COLOR_WIRE_LOW,
                        None    => COLOR_WIRE,
                    }
                } else {
                    COLOR_WIRE
                };

                let is_hovered = hovered_wire.as_ref() == Some(wire);
                let line_width = if is_hovered { 4.5 } else { 2.5 };
                let color = if is_hovered {
                    Color32::from_rgb(255, 80, 80)
                } else {
                    wire_color
                };
                draw_bezier_wire(painter, from_pos, to_pos, color, line_width);
            }
        }
    }

    fn draw_gate_node(
        &self,
        gate_index: usize,
        painter: &Painter,
        canvas_origin: Pos2,
        canvas_rect: Rect,
        is_hovered: bool,
        is_dragging: bool,
    ) {
        let node = &self.graph.nodes[gate_index];
        let node_height = compute_node_height(node.input_count, node.output_count);
        let top_left_screen = self.canvas_to_screen(node.pos, canvas_origin);
        let node_rect = Rect::from_min_size(
            top_left_screen,
            vec2(NODE_WIDTH * self.canvas_zoom, node_height * self.canvas_zoom),
        );

        let fill_color = if is_dragging {
            Color32::from_rgb(68, 74, 110)
        } else if is_hovered {
            COLOR_NODE_HOVERED
        } else {
            COLOR_NODE_FILL
        };
        let border_color = if is_dragging || is_hovered { Color32::WHITE } else { COLOR_NODE_STROKE };
        painter.rect(node_rect, Rounding::same(5.0 * self.canvas_zoom), fill_color, Stroke::new(1.5, border_color));
        painter.text(
            node_rect.center_top() + vec2(0.0, 13.0 * self.canvas_zoom),
            Align2::CENTER_CENTER,
            &node.label,
            FontId::proportional(12.0 * self.canvas_zoom),
            COLOR_TEXT,
        );

        let node_id = self.graph.gate_node_id(gate_index);

        for port_index in 0..node.input_count {
            let canvas_pos = input_port_canvas_pos(node, port_index);
            let screen_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
            // Color the input port by the signal currently on the wire driving it.
            let port_color = self.live_port_color_for_input(node_id, port_index);
            painter.circle_filled(screen_pos, PORT_RADIUS * self.canvas_zoom, port_color);
            painter.circle_stroke(screen_pos, PORT_RADIUS * self.canvas_zoom, Stroke::new(1.0, Color32::WHITE));
        }

        for port_index in 0..node.output_count {
            let canvas_pos = output_port_canvas_pos(node, port_index);
            let screen_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
            let port_color = self.live_port_color_for_output(node_id, port_index);
            painter.circle_filled(screen_pos, PORT_RADIUS * self.canvas_zoom, port_color);
            painter.circle_stroke(screen_pos, PORT_RADIUS * self.canvas_zoom, Stroke::new(1.0, Color32::WHITE));
        }
    }

    /// Signal-aware color for an input port (looks up the wire driving it).
    fn live_port_color_for_input(&self, node_id: usize, port_index: usize) -> Color32 {
        if self.simulation.is_none() {
            return COLOR_PORT_INPUT;
        }
        // Find which wire drives this input port.
        let driving_wire = self.graph.wires.iter().find(|wire| {
            wire.to.node == node_id && wire.to.port == port_index
        });
        let Some(wire) = driving_wire else { return COLOR_PORT_INPUT };
        let Some(&wire_idx) = self.port_to_wire_index.get(&(wire.from.node, wire.from.port, true)) else {
            return COLOR_PORT_INPUT;
        };
        if self.live_wire_signals.get(&wire_idx).copied().unwrap_or(false) {
            COLOR_SIGNAL_HIGH
        } else {
            COLOR_SIGNAL_LOW
        }
    }

    /// Signal-aware color for an output port.
    fn live_port_color_for_output(&self, node_id: usize, port_index: usize) -> Color32 {
        if self.simulation.is_none() {
            return COLOR_PORT_OUTPUT;
        }
        let Some(&wire_idx) = self.port_to_wire_index.get(&(node_id, port_index, true)) else {
            return COLOR_PORT_OUTPUT;
        };
        if self.live_wire_signals.get(&wire_idx).copied().unwrap_or(false) {
            COLOR_SIGNAL_HIGH
        } else {
            COLOR_SIGNAL_LOW
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Coordinate helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn canvas_to_screen(&self, canvas_pos: Pos2, canvas_origin: Pos2) -> Pos2 {
        canvas_origin + (canvas_pos.to_vec2() + self.canvas_pan) * self.canvas_zoom
    }
    fn screen_to_canvas(&self, screen_pos: Pos2, canvas_origin: Pos2) -> Pos2 {
        ((screen_pos - canvas_origin) / self.canvas_zoom - self.canvas_pan).to_pos2()
    }

    fn port_to_screen_pos(
        &self,
        port: &PortRef,
        is_output_port: bool,
        canvas_origin: Pos2,
        canvas_rect: Rect,
    ) -> Option<Pos2> {
        let input_count  = self.graph.inputs.len();
        let output_count = self.graph.outputs.len();
        let center_y = canvas_rect.center().y;

        if self.graph.is_input_node(port.node) {
            let start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            return Some(pos2(canvas_rect.left() + 14.0, start_y + port.node as f32 * IO_RAIL_STEP));
        }
        if self.graph.is_output_node(port.node) {
            let output_index = port.node - input_count;
            let start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            return Some(pos2(canvas_rect.right() - 14.0, start_y + output_index as f32 * IO_RAIL_STEP));
        }
        if let Some(gate_index) = self.graph.gate_index_from_node_id(port.node) {
            let node = &self.graph.nodes[gate_index];
            let canvas_pos = if is_output_port {
                output_port_canvas_pos(node, port.port)
            } else {
                input_port_canvas_pos(node, port.port)
            };
            return Some(self.canvas_to_screen(canvas_pos, canvas_origin));
        }
        None
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Hit testing
    // ─────────────────────────────────────────────────────────────────────────

    fn hit_test_port(
        &self,
        screen_pos: Pos2,
        canvas_origin: Pos2,
        canvas_rect: Rect,
    ) -> Option<(PortRef, bool)> {
        let input_count  = self.graph.inputs.len();
        let output_count = self.graph.outputs.len();
        let hit_radius = (PORT_RADIUS + 6.0) * self.canvas_zoom;
        let center_y = canvas_rect.center().y;

        let inputs_start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for input_index in 0..input_count {
            let dot_pos = pos2(canvas_rect.left() + 14.0, inputs_start_y + input_index as f32 * IO_RAIL_STEP);
            if (screen_pos - dot_pos).length() < hit_radius {
                return Some((PortRef { node: self.graph.input_node_id(input_index), port: 0 }, true));
            }
        }

        let outputs_start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for output_index in 0..output_count {
            let dot_pos = pos2(canvas_rect.right() - 14.0, outputs_start_y + output_index as f32 * IO_RAIL_STEP);
            if (screen_pos - dot_pos).length() < hit_radius {
                return Some((PortRef { node: self.graph.output_node_id(output_index), port: 0 }, false));
            }
        }

        for gate_index in 0..self.graph.nodes.len() {
            let node = &self.graph.nodes[gate_index];
            for port_index in 0..node.input_count {
                let dot_pos = self.canvas_to_screen(input_port_canvas_pos(node, port_index), canvas_origin);
                if (screen_pos - dot_pos).length() < hit_radius {
                    return Some((PortRef { node: self.graph.gate_node_id(gate_index), port: port_index }, false));
                }
            }
            for port_index in 0..node.output_count {
                let dot_pos = self.canvas_to_screen(output_port_canvas_pos(node, port_index), canvas_origin);
                if (screen_pos - dot_pos).length() < hit_radius {
                    return Some((PortRef { node: self.graph.gate_node_id(gate_index), port: port_index }, true));
                }
            }
        }

        None
    }

    fn hit_test_gate(&self, screen_pos: Pos2, canvas_origin: Pos2) -> Option<usize> {
        for gate_index in (0..self.graph.nodes.len()).rev() {
            let node = &self.graph.nodes[gate_index];
            let node_height = compute_node_height(node.input_count, node.output_count);
            let top_left_screen = self.canvas_to_screen(node.pos, canvas_origin);
            let node_rect = Rect::from_min_size(
                top_left_screen,
                vec2(NODE_WIDTH * self.canvas_zoom, node_height * self.canvas_zoom),
            );
            if node_rect.contains(screen_pos) { return Some(gate_index); }
        }
        None
    }

    /// Returns the wire under the cursor by checking proximity to each bezier midpoint.
    fn hit_test_wire(
        &self,
        screen_pos: Pos2,
        canvas_origin: Pos2,
        canvas_rect: Rect,
    ) -> Option<Wire> {
        let hit_radius = 8.0 * self.canvas_zoom;
        for wire in &self.graph.wires {
            let from_screen = self.port_to_screen_pos(&wire.from, true,  canvas_origin, canvas_rect);
            let to_screen   = self.port_to_screen_pos(&wire.to,   false, canvas_origin, canvas_rect);
            if let (Some(from_pos), Some(to_pos)) = (from_screen, to_screen) {
                // Sample several points along the bezier and test proximity.
                let horizontal_ctrl_offset = ((to_pos.x - from_pos.x).abs() * 0.45).max(60.0);
                let ctrl1 = pos2(from_pos.x + horizontal_ctrl_offset, from_pos.y);
                let ctrl2 = pos2(to_pos.x   - horizontal_ctrl_offset, to_pos.y);
                for sample_step in 0..=16usize {
                    let t = sample_step as f32 / 16.0;
                    let u = 1.0 - t;
                    let sample_pos = pos2(
                        u*u*u*from_pos.x + 3.0*u*u*t*ctrl1.x + 3.0*u*t*t*ctrl2.x + t*t*t*to_pos.x,
                        u*u*u*from_pos.y + 3.0*u*u*t*ctrl1.y + 3.0*u*t*t*ctrl2.y + t*t*t*to_pos.y,
                    );
                    if (screen_pos - sample_pos).length() < hit_radius {
                        return Some(wire.clone());
                    }
                }
            }
        }
        None
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Simulation
    // ─────────────────────────────────────────────────────────────────────────

    fn build_simulation_from_graph(&mut self) {
        let current_desc = editor_graph_to_desc(&self.graph);
        let library_descs: Vec<GraphDesc> = self
            .library
            .iter()
            .map(|saved_gate| editor_graph_to_desc(&saved_gate.graph))
            .collect();

        // Build the port→wire_index map alongside the simulation so we can
        // visualize live signal state without needing to access WireId directly.
        let port_to_wire_index = build_port_to_wire_index_map(&current_desc);

        match build_simulation(&current_desc, &library_descs) {
            Ok(simulation) => {
                self.simulation = Some(simulation);
                self.port_to_wire_index = port_to_wire_index;
                self.simulation_error = None;
            }
            Err(error_message) => {
                self.simulation = None;
                self.port_to_wire_index = HashMap::new();
                self.simulation_error = Some(error_message);
            }
        }
        self.live_wire_signals = HashMap::new();
        self.output_states = vec![false; self.graph.outputs.len()];
    }

    fn step_simulation(&mut self) {
        let Some(simulation) = &mut self.simulation else { return };
        for (input_index, &input_value) in self.input_states.iter().enumerate() {
            if input_index < simulation.input_wires.len() {
                simulation.force_set_wire(simulation.input_wires[input_index], input_value);
            }
        }
        simulation.run_one_tick();
        self.output_states = simulation.outputs().collect();

        // Snapshot live wire values for all known output ports so the renderer can
        // color ports & wires.  We use the port_to_wire_index map (built from the
        // same wire-assignment order as build_simulation) to enumerate every wire.
        self.live_wire_signals.clear();
        for &wire_index in self.port_to_wire_index.values() {
            let signal = read_wire_by_index(simulation, wire_index);
            self.live_wire_signals.insert(wire_index, signal);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Actions
    // ─────────────────────────────────────────────────────────────────────────

    fn save_current_graph_to_library(&mut self) {
        let gate = LibraryGate {
            name: self.title.clone(),
            input_count: self.graph.inputs.len(),
            output_count: self.graph.outputs.len(),
            graph: self.graph.clone(),
        };
        if let Some(existing) = self.library.iter_mut().find(|saved| saved.name == gate.name) {
            *existing = gate;
        } else {
            self.library.push(gate);
        }
    }

    fn save_library_to_file(&mut self) {
        fn fallible_save(lib: &Vec<LibraryGate>) -> Result<(), &'static str> {
            bincode::serialize_into(
                std::fs::File::create("my_library.logic_builder_lib")
                    .map_err(|_| "failed to create or open file to save library")?,
                &lib,
            )
            .map_err(|_| "failed to serialize library for saving")
        }
        match fallible_save(&self.library) {
            Ok(_) => {}
            Err(err) => self.simulation_error = Some(err.to_string()),
        }
    }

    fn load_library_from_file(&mut self) {
        fn fallible_load() -> Result<Vec<LibraryGate>, &'static str> {
            bincode::deserialize_from(
                std::fs::File::open("my_library.logic_builder_lib")
                    .map_err(|_| "failed to open file to load library")?,
            )
            .map_err(|_| "failed to deserialize library on load")
        }
        match fallible_load() {
            Ok(library) => self.library = library,
            Err(err) => self.simulation_error = Some(err.to_string()),
        }
    }

    fn open_library_gate_for_editing(&mut self, library_index: usize) {
        let gate = self.library[library_index].clone();
        self.title              = gate.name;
        self.graph              = gate.graph;
        self.input_states       = vec![false; gate.input_count];
        self.output_states      = vec![false; gate.output_count];
        self.simulation         = None;
        self.simulation_error   = None;
        self.simulation_running = false;
        self.pending_wire_start = None;
        self.dragging_gate      = None;
        self.live_wire_signals  = HashMap::new();
        self.port_to_wire_index = HashMap::new();
    }

    /// Delete a library gate by index, remapping any SavedGate references in all
    /// stored graphs so the indices stay consistent.
    fn delete_library_gate(&mut self, deleted_index: usize) {
        self.library.remove(deleted_index);

        // Remap SavedGate(idx) references in every graph stored in the library,
        // and in the active canvas.
        let remap_gate_kind = |mut kind: &mut EditorNodeKind| {
            if let EditorNodeKind::SavedGate(idx) = kind {
                if *idx == deleted_index {
                    // This instance now refers to a deleted gate — clear it back to NAND
                    // as a safe fallback (the user will see the node become "NAND").
                    *kind = EditorNodeKind::Nand;
                } else if *idx > deleted_index {
                    *idx -= 1;
                }
            }
        };

        for node in &mut self.graph.nodes {
            remap_gate_kind(&mut node.kind);
        }
        for lib_gate in &mut self.library {
            for node in &mut lib_gate.graph.nodes {
                remap_gate_kind(&mut node.kind);
            }
        }

        // Clear rename state if it pointed at the deleted gate or a shifted index.
        self.library_rename_index = None;
        self.library_rename_text.clear();
    }

    fn clear_canvas(&mut self) {
        self.graph              = EditorGraph::default();
        self.simulation         = None;
        self.simulation_error   = None;
        self.simulation_running = false;
        self.input_states       = vec![false];
        self.output_states      = vec![false];
        self.pending_wire_start = None;
        self.dragging_gate      = None;
        self.live_wire_signals  = HashMap::new();
        self.port_to_wire_index = HashMap::new();
        self.bulk_wire_state    = BulkWireState::Idle;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  EditorGraph → GraphDesc
// ─────────────────────────────────────────────────────────────────────────────

fn editor_graph_to_desc(graph: &EditorGraph) -> GraphDesc {
    let input_count  = graph.inputs.len();
    let output_count = graph.outputs.len();
    GraphDesc {
        n_inputs:    input_count,
        n_outputs:   output_count,
        input_base:  0,
        output_base: input_count,
        gate_base:   input_count + output_count,
        gates: graph.nodes.iter().map(|node| {
            let kind = match &node.kind {
                EditorNodeKind::Nand           => GateKind::Nand,
                EditorNodeKind::SavedGate(idx) => GateKind::SavedGate(*idx),
            };
            (node.input_count, node.output_count, kind)
        }).collect(),
        wires: graph.wires.iter().map(|wire| WireDesc {
            from: wire.from.clone(),
            to:   wire.to.clone(),
        }).collect(),
    }
}

/// Build a map from (node_id, port_index, is_output) → wire_index,
/// using the same wire-assignment logic as `build_simulation`.
/// This lets main.rs look up signal state without touching WireId directly.
fn build_port_to_wire_index_map(desc: &GraphDesc) -> HashMap<(usize, usize, bool), u32> {
    let mut port_to_wire: HashMap<(usize, usize, bool), u32> = HashMap::new();
    let mut next_wire_id: u32 = 0;

    // Input pseudo-nodes each have one output port.
    for input_index in 0..desc.n_inputs {
        let node_id = desc.input_base + input_index;
        port_to_wire.insert((node_id, 0, true), next_wire_id);
        next_wire_id += 1;
    }

    // Internal gate output ports.
    for (gate_slot, (_, gate_output_count, _)) in desc.gates.iter().enumerate() {
        let node_id = desc.gate_base + gate_slot;
        for port_index in 0..*gate_output_count {
            port_to_wire.insert((node_id, port_index, true), next_wire_id);
            next_wire_id += 1;
        }
    }

    port_to_wire
}

// ─────────────────────────────────────────────────────────────────────────────
//  Geometry helpers
// ─────────────────────────────────────────────────────────────────────────────

fn compute_node_height(input_count: usize, output_count: usize) -> f32 {
    PORT_TOP_PADDING + PORT_VERTICAL_STEP * (input_count.max(output_count).max(1) as f32) + 10.0
}

fn input_port_canvas_pos(node: &EditorNode, port_index: usize) -> Pos2 {
    Pos2::new(node.pos.x, node.pos.y + PORT_TOP_PADDING + port_index as f32 * PORT_VERTICAL_STEP)
}

fn output_port_canvas_pos(node: &EditorNode, port_index: usize) -> Pos2 {
    Pos2::new(node.pos.x + NODE_WIDTH, node.pos.y + PORT_TOP_PADDING + port_index as f32 * PORT_VERTICAL_STEP)
}

fn snap_to_grid(pos: Pos2) -> Pos2 {
    Pos2::new((pos.x / GRID_CELL_SIZE).round() * GRID_CELL_SIZE, (pos.y / GRID_CELL_SIZE).round() * GRID_CELL_SIZE)
}

fn make_nand_node(pos: Pos2) -> EditorNode {
    EditorNode { label: "NAND".into(), pos, input_count: 2, output_count: 1, kind: EditorNodeKind::Nand }
}

/// Build a canvas-space Rect from any two opposite corners.
fn canvas_rect_from_two_points(corner_a: Pos2, corner_b: Pos2) -> Rect {
    Rect::from_min_max(
        Pos2::new(corner_a.x.min(corner_b.x), corner_a.y.min(corner_b.y)),
        Pos2::new(corner_a.x.max(corner_b.x), corner_a.y.max(corner_b.y)),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
//  Bezier wire rendering
// ─────────────────────────────────────────────────────────────────────────────

fn draw_bezier_wire(painter: &Painter, from: Pos2, to: Pos2, color: Color32, line_width: f32) {
    let horizontal_ctrl_offset = ((to.x - from.x).abs() * 0.45).max(60.0);
    let ctrl1 = pos2(from.x + horizontal_ctrl_offset, from.y);
    let ctrl2 = pos2(to.x   - horizontal_ctrl_offset, to.y);
    let mut prev_point = from;
    for step in 1..=32usize {
        let t = step as f32 / 32.0;
        let u = 1.0 - t;
        let next_point = pos2(
            u*u*u*from.x + 3.0*u*u*t*ctrl1.x + 3.0*u*t*t*ctrl2.x + t*t*t*to.x,
            u*u*u*from.y + 3.0*u*u*t*ctrl1.y + 3.0*u*t*t*ctrl2.y + t*t*t*to.y,
        );
        painter.line_segment([prev_point, next_point], Stroke::new(line_width, color));
        prev_point = next_point;
    }
}