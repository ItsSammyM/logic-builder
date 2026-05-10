#![allow(unused, clippy::all)]

use egui::*;

mod bit_array;
mod simulation;
mod sim_builder;

use serde::{Deserialize, Serialize, ser::SerializeStruct as _};
use sim_builder::{build_simulation, GateKind, GraphDesc, PortRef, WireDesc};
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
    where
        S: Serializer,
    {
        (pos.x, pos.y).serialize(s)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Pos2, D::Error>
    where
        D: Deserializer<'de>,
    {
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
}

// ─────────────────────────────────────────────────────────────────────────────
//  App
// ─────────────────────────────────────────────────────────────────────────────

struct App {
    title: String,
    graph: EditorGraph,
    library: Vec<LibraryGate>,

    simulation: Option<Simulation>,
    input_states: Vec<bool>,
    output_states: Vec<bool>,
    simulation_running: bool,
    simulation_error: Option<String>,

    canvas_pan: Vec2,
    canvas_zoom: f32,

    /// The output port the user clicked to start drawing a wire from.
    pending_wire_start: Option<PortRef>,
    /// Index of the gate being dragged, plus the offset from its top-left corner to the click point.
    dragging_gate: Option<(usize, Vec2)>,

    new_input_name: String,
    new_output_name: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            title: "My Gate".into(),
            graph: EditorGraph::default(),
            library: vec![],
            simulation: None,
            input_states: vec![false],
            output_states: vec![false],
            simulation_running: false,
            simulation_error: None,
            canvas_pan: Vec2::ZERO,
            canvas_zoom: 1.0,
            pending_wire_start: None,
            dragging_gate: None,
            new_input_name: String::new(),
            new_output_name: String::new(),
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

const COLOR_BACKGROUND: Color32   = Color32::from_rgb(22, 24, 34);
const COLOR_GRID: Color32         = Color32::from_rgb(38, 42, 58);
const COLOR_PANEL_BG: Color32     = Color32::from_rgb(28, 30, 42);
const COLOR_NODE_FILL: Color32    = Color32::from_rgb(48, 52, 76);
const COLOR_NODE_HOVERED: Color32 = Color32::from_rgb(68, 74, 110);
const COLOR_NODE_STROKE: Color32  = Color32::from_rgb(90, 100, 150);
const COLOR_PORT_INPUT: Color32   = Color32::from_rgb(80, 175, 235);
const COLOR_PORT_OUTPUT: Color32  = Color32::from_rgb(235, 175, 60);
const COLOR_WIRE: Color32         = Color32::from_rgb(130, 220, 110);
const COLOR_WIRE_PENDING: Color32 = Color32::from_rgb(240, 220, 60);
const COLOR_SIGNAL_HIGH: Color32  = Color32::from_rgb(70, 230, 90);
const COLOR_SIGNAL_LOW: Color32   = Color32::from_rgb(55, 60, 85);
const COLOR_TEXT: Color32         = Color32::from_rgb(210, 218, 255);
const COLOR_DIM: Color32          = Color32::from_rgb(120, 130, 170);

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
                        .button(
                            RichText::new("💾  Save Gate")
                                .color(Color32::from_rgb(150, 210, 255)),
                        )
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

                    if ui
                        .button("⏭  Step")
                        .on_hover_text("Rebuild & advance one tick")
                        .clicked()
                    {
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
                            RichText::new(if self.simulation_running {
                                "● Running"
                            } else {
                                "● Built"
                            })
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
                        let toggle_label = if is_on { "🟢" } else { "⚫" };
                        if ui.small_button(toggle_label).clicked() {
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
                    let node_id = self.graph.input_node_id(index);
                    self.graph.wires.retain(|wire| wire.from.node != node_id);
                    self.graph.inputs.remove(index);
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

                ui.add_space(20.0);
                for hint_text in &[
                    "Right-click canvas\nto spawn gates",
                    "Click output port\n(yellow) then input\nport (blue) to wire",
                    "Drag nodes to move",
                    "Middle-drag or scroll\nto pan & zoom",
                    "Hover + Del to\ndelete a gate",
                ] {
                    ui.label(RichText::new(*hint_text).color(COLOR_DIM).size(10.5));
                    ui.add_space(4.0);
                }
            });
    }

    // ── right panel — outputs & library ──────────────────────────────────────

    fn show_right_panel(&mut self, ctx: &egui::Context) {
        SidePanel::right("right_panel")
            .resizable(false)
            .exact_width(158.0)
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
                    let node_id = self.graph.output_node_id(index);
                    self.graph.wires.retain(|wire| wire.to.node != node_id);
                    self.graph.outputs.remove(index);
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

                // ── library ───────────────────────────────────────────────

                ui.add_space(16.0);
                    ui.label(RichText::new("LIBRARY").color(COLOR_DIM).strong().size(11.0));
                    ui.label(
                        RichText::new("Click a gate to open it")
                            .color(COLOR_DIM)
                            .italics()
                            .size(10.0),
                    );
                    ui.separator();

                if ui
                    .button(
                        RichText::new("Load Library")
                            .color(Color32::from_rgb(150, 210, 255)),
                    )
                    .on_hover_text("Load library from file")
                    .clicked()
                {
                    self.load_library_from_file();
                }


                if !self.library.is_empty() {
                    if ui
                        .button(
                            RichText::new("Save Library")
                                .color(Color32::from_rgb(150, 210, 255)),
                        )
                        .on_hover_text("Save library to file")
                        .clicked()
                    {
                        self.save_library_to_file();
                    }
                    

                    // Collect the clicked index before acting, so we don't borrow `self.library`
                    // and mutate `self` at the same time.
                    let mut library_gate_to_open: Option<usize> = None;

                    for (library_index, saved_gate) in self.library.iter().enumerate() {
                        let button_label = format!(
                            "▣ {}  ({} → {})",
                            saved_gate.name, saved_gate.input_count, saved_gate.output_count
                        );
                        if ui
                            .add(
                                Button::new(
                                    RichText::new(button_label)
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 200, 255)),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Click to open and edit")
                            .clicked()
                        {
                            library_gate_to_open = Some(library_index);
                        }
                    }

                    if let Some(library_index) = library_gate_to_open {
                        self.open_library_gate_for_editing(library_index);
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

                // ── pan (middle mouse) ─────────────────────────────────────
                if canvas_response.dragged_by(PointerButton::Middle) {
                    self.canvas_pan += canvas_response.drag_delta() / self.canvas_zoom;
                }

                let pointer_screen_pos =
                    ui.input(|input| input.pointer.interact_pos());

                let hovered_port =
                    pointer_screen_pos.and_then(|pos| self.hit_test_port(pos, canvas_origin, canvas_rect));
                let hovered_gate_index =
                    pointer_screen_pos.and_then(|pos| self.hit_test_gate(pos, canvas_origin));

                // ── end gate drag ─────────────────────────────────────────
                if canvas_response.drag_stopped() {
                    self.dragging_gate = None;
                }

                // ── continue gate drag ────────────────────────────────────
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
                // ── start gate drag ───────────────────────────────────────
                else if canvas_response.drag_started_by(PointerButton::Primary)
                    && hovered_port.is_none()
                {
                    if let (Some(gate_index), Some(pointer_pos)) =
                        (hovered_gate_index, pointer_screen_pos)
                    {
                        let drag_offset = self.graph.nodes[gate_index].pos
                            - self.screen_to_canvas(pointer_pos, canvas_origin);
                        self.dragging_gate = Some((gate_index, drag_offset));
                    }
                }

                // ── port click → wire ─────────────────────────────────────
                if canvas_response.clicked() {
                    if let Some((clicked_port, is_output_port)) = hovered_port.clone() {
                        match self.pending_wire_start.take() {
                            None => {
                                if is_output_port {
                                    self.pending_wire_start = Some(clicked_port);
                                }
                            }
                            Some(wire_start) => {
                                if !is_output_port {
                                    // Each input port can only have one driver.
                                    self.graph.wires.retain(|wire| {
                                        !(wire.to.node == clicked_port.node
                                            && wire.to.port == clicked_port.port)
                                    });
                                    self.graph.wires.push(Wire {
                                        from: wire_start,
                                        to: clicked_port,
                                    });
                                } else {
                                    // Clicked another output port → replace pending start.
                                    self.pending_wire_start = Some(clicked_port);
                                }
                            }
                        }
                    } else {
                        self.pending_wire_start = None;
                    }
                }

                // ── delete hovered gate ───────────────────────────────────
                if ui.input(|input| {
                    input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace)
                }) {
                    if let Some(gate_index) = hovered_gate_index {
                        let node_id = self.graph.gate_node_id(gate_index);
                        self.graph.wires.retain(|wire| {
                            wire.from.node != node_id && wire.to.node != node_id
                        });
                        self.graph.nodes.remove(gate_index);
                    }
                }

                // ── right-click context menu ──────────────────────────────
                let spawn_canvas_pos = pointer_screen_pos
                    .map(|pos| snap_to_grid(self.screen_to_canvas(pos, canvas_origin)))
                    .unwrap_or(Pos2::ZERO);

                canvas_response.context_menu(|ui| {
                    ui.label(RichText::new("Add Gate").strong().size(13.0));
                    ui.separator();

                    if ui.button("⊼  NAND  (2→1)").clicked() {
                        self.graph.nodes.push(make_nand_node(spawn_canvas_pos));
                        ui.close_menu();
                    }

                    if !self.library.is_empty() {
                        ui.separator();
                        ui.label(
                            RichText::new("Library")
                                .color(COLOR_DIM)
                                .italics()
                                .size(11.0),
                        );
                        let mut gate_to_spawn: Option<usize> = None;
                        for (library_index, saved_gate) in self.library.iter().enumerate() {
                            let button_label = format!(
                                "▣  {}  ({} → {})",
                                saved_gate.name,
                                saved_gate.input_count,
                                saved_gate.output_count
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

                // ─ draw ──────────────────────────────────────────────────
                self.draw_grid(&painter, canvas_rect);
                self.draw_io_rails(&painter, canvas_rect);
                self.draw_wires(&painter, canvas_origin, canvas_rect);

                // In-progress wire preview following the cursor.
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
                    self.draw_gate_node(gate_index, &painter, canvas_origin, is_hovered, is_dragging);
                }

                // Highlight ring on the port under the mouse.
                if let Some((hovered_port_ref, is_output_port)) = &hovered_port {
                    let screen_pos = self.port_to_screen_pos(
                        hovered_port_ref,
                        *is_output_port,
                        canvas_origin,
                        canvas_rect,
                    );
                    if let Some(pos) = screen_pos {
                        let highlight_color =
                            if *is_output_port { COLOR_PORT_OUTPUT } else { COLOR_PORT_INPUT };
                        painter.circle_stroke(
                            pos,
                            PORT_RADIUS * self.canvas_zoom * 2.0,
                            Stroke::new(2.0, highlight_color),
                        );
                    }
                }
            });
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

        // Input dots on the left edge.
        let inputs_start_y =
            center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for (input_index, input_name) in self.graph.inputs.iter().enumerate() {
            let screen_pos = pos2(
                rect.left() + 14.0,
                inputs_start_y + input_index as f32 * IO_RAIL_STEP,
            );
            let is_on = self.input_states.get(input_index).copied().unwrap_or(false);
            painter.circle_filled(screen_pos, PORT_RADIUS + 3.0, if is_on { COLOR_SIGNAL_HIGH } else { COLOR_SIGNAL_LOW });
            painter.circle_stroke(screen_pos, PORT_RADIUS + 3.0, Stroke::new(1.5, COLOR_PORT_INPUT));
            painter.text(
                screen_pos + vec2(14.0, 0.0),
                Align2::LEFT_CENTER,
                input_name,
                FontId::proportional(12.0),
                COLOR_TEXT,
            );
        }

        // Output dots on the right edge.
        let outputs_start_y =
            center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for (output_index, output_name) in self.graph.outputs.iter().enumerate() {
            let screen_pos = pos2(
                rect.right() - 14.0,
                outputs_start_y + output_index as f32 * IO_RAIL_STEP,
            );
            let is_on = self.output_states.get(output_index).copied().unwrap_or(false);
            painter.circle_filled(screen_pos, PORT_RADIUS + 3.0, if is_on { COLOR_SIGNAL_HIGH } else { COLOR_SIGNAL_LOW });
            painter.circle_stroke(screen_pos, PORT_RADIUS + 3.0, Stroke::new(1.5, COLOR_PORT_OUTPUT));
            painter.text(
                screen_pos - vec2(14.0, 0.0),
                Align2::RIGHT_CENTER,
                output_name,
                FontId::proportional(12.0),
                COLOR_TEXT,
            );
        }
    }

    fn draw_wires(&self, painter: &Painter, canvas_origin: Pos2, canvas_rect: Rect) {
        for wire in &self.graph.wires {
            let from_screen =
                self.port_to_screen_pos(&wire.from, true,  canvas_origin, canvas_rect);
            let to_screen =
                self.port_to_screen_pos(&wire.to,   false, canvas_origin, canvas_rect);
            if let (Some(from_pos), Some(to_pos)) = (from_screen, to_screen) {
                draw_bezier_wire(painter, from_pos, to_pos, COLOR_WIRE, 2.5);
            }
        }
    }

    fn draw_gate_node(
        &self,
        gate_index: usize,
        painter: &Painter,
        canvas_origin: Pos2,
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
        let border_color = if is_dragging || is_hovered {
            Color32::WHITE
        } else {
            COLOR_NODE_STROKE
        };

        painter.rect(
            node_rect,
            Rounding::same(5.0 * self.canvas_zoom),
            fill_color,
            Stroke::new(1.5, border_color),
        );

        painter.text(
            node_rect.center_top() + vec2(0.0, 13.0 * self.canvas_zoom),
            Align2::CENTER_CENTER,
            &node.label,
            FontId::proportional(12.0 * self.canvas_zoom),
            COLOR_TEXT,
        );

        for port_index in 0..node.input_count {
            let canvas_pos = input_port_canvas_pos(node, port_index);
            let screen_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
            painter.circle_filled(screen_pos, PORT_RADIUS * self.canvas_zoom, COLOR_PORT_INPUT);
            painter.circle_stroke(
                screen_pos,
                PORT_RADIUS * self.canvas_zoom,
                Stroke::new(1.0, Color32::WHITE),
            );
        }

        for port_index in 0..node.output_count {
            let canvas_pos = output_port_canvas_pos(node, port_index);
            let screen_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
            painter.circle_filled(screen_pos, PORT_RADIUS * self.canvas_zoom, COLOR_PORT_OUTPUT);
            painter.circle_stroke(
                screen_pos,
                PORT_RADIUS * self.canvas_zoom,
                Stroke::new(1.0, Color32::WHITE),
            );
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

    /// Returns the screen-space position of a port dot.
    /// `is_output_port` selects left (input) vs right (output) side for gate nodes.
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
            return Some(pos2(
                canvas_rect.left() + 14.0,
                start_y + port.node as f32 * IO_RAIL_STEP,
            ));
        }

        if self.graph.is_output_node(port.node) {
            let output_index = port.node - input_count;
            let start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            return Some(pos2(
                canvas_rect.right() - 14.0,
                start_y + output_index as f32 * IO_RAIL_STEP,
            ));
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

    /// Returns `(port_ref, is_output_port)` for the port under the cursor, if any.
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

        // Input rail dots (they are output ports in graph terms — they emit the signal).
        let inputs_start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for input_index in 0..input_count {
            let dot_pos = pos2(
                canvas_rect.left() + 14.0,
                inputs_start_y + input_index as f32 * IO_RAIL_STEP,
            );
            if (screen_pos - dot_pos).length() < hit_radius {
                return Some((
                    PortRef { node: self.graph.input_node_id(input_index), port: 0 },
                    true,
                ));
            }
        }

        // Output rail dots (they are input ports in graph terms — they consume the signal).
        let outputs_start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
        for output_index in 0..output_count {
            let dot_pos = pos2(
                canvas_rect.right() - 14.0,
                outputs_start_y + output_index as f32 * IO_RAIL_STEP,
            );
            if (screen_pos - dot_pos).length() < hit_radius {
                return Some((
                    PortRef { node: self.graph.output_node_id(output_index), port: 0 },
                    false,
                ));
            }
        }

        // Gate node ports.
        for gate_index in 0..self.graph.nodes.len() {
            let node = &self.graph.nodes[gate_index];
            for port_index in 0..node.input_count {
                let canvas_pos = input_port_canvas_pos(node, port_index);
                let dot_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
                if (screen_pos - dot_pos).length() < hit_radius {
                    return Some((
                        PortRef { node: self.graph.gate_node_id(gate_index), port: port_index },
                        false,
                    ));
                }
            }
            for port_index in 0..node.output_count {
                let canvas_pos = output_port_canvas_pos(node, port_index);
                let dot_pos = self.canvas_to_screen(canvas_pos, canvas_origin);
                if (screen_pos - dot_pos).length() < hit_radius {
                    return Some((
                        PortRef { node: self.graph.gate_node_id(gate_index), port: port_index },
                        true,
                    ));
                }
            }
        }

        None
    }

    /// Returns the index into `self.graph.nodes` of the topmost gate under the screen position.
    fn hit_test_gate(&self, screen_pos: Pos2, canvas_origin: Pos2) -> Option<usize> {
        for gate_index in (0..self.graph.nodes.len()).rev() {
            let node = &self.graph.nodes[gate_index];
            let node_height = compute_node_height(node.input_count, node.output_count);
            let top_left_screen = self.canvas_to_screen(node.pos, canvas_origin);
            let node_rect = Rect::from_min_size(
                top_left_screen,
                vec2(NODE_WIDTH * self.canvas_zoom, node_height * self.canvas_zoom),
            );
            if node_rect.contains(screen_pos) {
                return Some(gate_index);
            }
        }
        None
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Simulation
    // ─────────────────────────────────────────────────────────────────────────

    fn build_simulation_from_graph(&mut self) {
        let current_desc = editor_graph_to_desc(&self.graph);
        // Build a GraphDesc for every library gate so recursive compilation works.
        let library_descs: Vec<GraphDesc> = self
            .library
            .iter()
            .map(|saved_gate| editor_graph_to_desc(&saved_gate.graph))
            .collect();

        match build_simulation(&current_desc, &library_descs) {
            Ok(simulation) => {
                self.simulation = Some(simulation);
                self.simulation_error = None;
            }
            Err(error_message) => {
                self.simulation = None;
                self.simulation_error = Some(error_message);
            }
        }
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
                std::fs::File::create("my_library.logic_builder_lib").map_err(|_|"failed to create or open file to save library")?,
                &lib,
            ).map_err(|_|"failed to serialize library for saving")
        }
        match fallible_save(&self.library) {
            Ok(_) => (),
            Err(err) => self.simulation_error = Some(err.to_string()),
        }
    }
    fn load_library_from_file(&mut self) {
        fn fallible_load() -> Result<Vec<LibraryGate>, &'static str> {
            bincode::deserialize_from(std::fs::File::open("my_library.logic_builder_lib")
                .map_err(|_|"failed to open file to load library")?)
                .map_err(|_|"failed to deserialize library on load")
        }
        match fallible_load() {
            Ok(library) => self.library = library,
            Err(err) => self.simulation_error = Some(err.to_string()),
        }
    }

    /// Load a saved gate's graph into the editor canvas for inspection or modification.
    fn open_library_gate_for_editing(&mut self, library_index: usize) {
        let gate = self.library[library_index].clone();
        self.title             = gate.name;
        self.graph             = gate.graph;
        self.input_states      = vec![false; gate.input_count];
        self.output_states     = vec![false; gate.output_count];
        self.simulation        = None;
        self.simulation_error  = None;
        self.simulation_running = false;
        self.pending_wire_start = None;
        self.dragging_gate     = None;
    }

    fn clear_canvas(&mut self) {
        self.graph = EditorGraph::default();
        self.simulation        = None;
        self.simulation_error  = None;
        self.simulation_running = false;
        self.input_states      = vec![false];
        self.output_states     = vec![false];
        self.pending_wire_start = None;
        self.dragging_gate     = None;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  EditorGraph → GraphDesc (also used by sim_builder for recursive compilation)
// ─────────────────────────────────────────────────────────────────────────────

fn editor_graph_to_desc(graph: &EditorGraph) -> GraphDesc {
    let input_count  = graph.inputs.len();
    let output_count = graph.outputs.len();
    GraphDesc {
        n_inputs:  input_count,
        n_outputs: output_count,
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

// ─────────────────────────────────────────────────────────────────────────────
//  Geometry helpers (free functions)
// ─────────────────────────────────────────────────────────────────────────────

fn compute_node_height(input_count: usize, output_count: usize) -> f32 {
    PORT_TOP_PADDING
        + PORT_VERTICAL_STEP * (input_count.max(output_count).max(1) as f32)
        + 10.0
}

fn input_port_canvas_pos(node: &EditorNode, port_index: usize) -> Pos2 {
    Pos2::new(
        node.pos.x,
        node.pos.y + PORT_TOP_PADDING + port_index as f32 * PORT_VERTICAL_STEP,
    )
}

fn output_port_canvas_pos(node: &EditorNode, port_index: usize) -> Pos2 {
    Pos2::new(
        node.pos.x + NODE_WIDTH,
        node.pos.y + PORT_TOP_PADDING + port_index as f32 * PORT_VERTICAL_STEP,
    )
}

fn snap_to_grid(pos: Pos2) -> Pos2 {
    Pos2::new(
        (pos.x / GRID_CELL_SIZE).round() * GRID_CELL_SIZE,
        (pos.y / GRID_CELL_SIZE).round() * GRID_CELL_SIZE,
    )
}

fn make_nand_node(pos: Pos2) -> EditorNode {
    EditorNode {
        label: "NAND".into(),
        pos,
        input_count: 2,
        output_count: 1,
        kind: EditorNodeKind::Nand,
    }
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