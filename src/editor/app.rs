use std::collections::HashMap;

use egui::Vec2;

use crate::sim_builder::PortRef;
use crate::simulation::simulation::Simulation;

use super::graph::{BulkWireState, EditorGraph, LibraryGate};

pub struct App {
    pub title: String,
    pub graph: EditorGraph,
    pub library: Vec<LibraryGate>,

    pub simulation: Option<Simulation>,
    pub live_wire_signals: HashMap<u32, bool>,
    pub port_to_wire_index: HashMap<(usize, usize, bool), u32>,
    pub input_states: Vec<bool>,
    pub output_states: Vec<bool>,
    pub simulation_running: bool,
    pub simulation_error: Option<String>,

    pub canvas_pan: Vec2,
    pub canvas_zoom: f32,

    pub pending_wire_start: Option<PortRef>,
    pub dragging_gate: Option<(usize, Vec2)>,

    pub bulk_wire_state: BulkWireState,

    pub new_input_name: String,
    pub new_output_name: String,

    pub library_rename_index: Option<usize>,
    pub library_rename_text: String,

    pub input_drag_reorder: Option<(usize, usize)>,
    pub output_drag_reorder: Option<(usize, usize)>,
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
            input_drag_reorder: None,
            output_drag_reorder: None,
        }
    }
}

impl App {
    /// Reset the canvas to a blank single-input / single-output graph.
    pub fn clear_canvas(&mut self) {
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