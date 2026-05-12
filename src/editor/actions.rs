use std::collections::HashMap;

use super::app::App;
use super::graph::{BulkWireState, EditorGraph, EditorNodeKind, LibraryGate};

impl App {
    // ─────────────────────────────────────────────────────────────────────────
    //  Library persistence
    // ─────────────────────────────────────────────────────────────────────────

    /// Save the current canvas as a `LibraryGate`.
    ///
    /// If a gate with the same name already exists it is overwritten in place;
    /// otherwise a new entry is appended.
    pub fn save_current_graph_to_library(&mut self) {
        let gate = LibraryGate {
            name:         self.title.clone(),
            input_count:  self.graph.inputs.len(),
            output_count: self.graph.outputs.len(),
            graph:        self.graph.clone(),
        };
        if let Some(existing) = self.library.iter_mut().find(|saved| saved.name == gate.name) {
            *existing = gate;
        } else {
            self.library.push(gate);
        }
    }

    pub fn save_library_to_file(&mut self) {
        fn fallible_save(library: &Vec<LibraryGate>) -> Result<(), &'static str> {
            bincode::serialize_into(
                std::fs::File::create("my_library.logic_builder_lib")
                    .map_err(|_| "failed to create or open file to save library")?,
                &library,
            )
            .map_err(|_| "failed to serialize library for saving")
        }
        if let Err(error) = fallible_save(&self.library) {
            self.simulation_error = Some(error.to_string());
        }
    }

    pub fn load_library_from_file(&mut self) {
        fn fallible_load() -> Result<Vec<LibraryGate>, &'static str> {
            bincode::deserialize_from(
                std::fs::File::open("my_library.logic_builder_lib")
                    .map_err(|_| "failed to open file to load library")?,
            )
            .map_err(|_| "failed to deserialize library on load")
        }
        match fallible_load() {
            Ok(library) => self.library = library,
            Err(error)  => self.simulation_error = Some(error.to_string()),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Library gate management
    // ─────────────────────────────────────────────────────────────────────────

    /// Load a saved gate back onto the canvas for editing.
    pub fn open_library_gate_for_editing(&mut self, library_index: usize) {
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

    /// Delete a library gate by index.
    ///
    /// Any `SavedGate(idx)` references in all stored graphs (including the active
    /// canvas) are remapped: indices above `deleted_index` are decremented by one,
    /// and references to exactly `deleted_index` are reset to `Nand` as a safe fallback.
    pub fn delete_library_gate(&mut self, deleted_index: usize) {
        self.library.remove(deleted_index);

        let remap_node_kind = |kind: &mut EditorNodeKind| {
            if let EditorNodeKind::SavedGate(idx) = kind {
                if *idx == deleted_index {
                    *kind = EditorNodeKind::Nand;
                } else if *idx > deleted_index {
                    *idx -= 1;
                }
            }
        };

        for node in &mut self.graph.nodes {
            remap_node_kind(&mut node.kind);
        }
        for library_gate in &mut self.library {
            for node in &mut library_gate.graph.nodes {
                remap_node_kind(&mut node.kind);
            }
        }

        // Clear rename state — it may now point at a stale index.
        self.library_rename_index = None;
        self.library_rename_text.clear();
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Canvas
    // ─────────────────────────────────────────────────────────────────────────

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
