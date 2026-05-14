use std::collections::HashMap;

use bincode::Options;

use super::app::App;
use super::graph::{EditorGraph, EditorNodeKind, LibraryGate};


impl LibraryGate{
    fn from_editor(app: &App)->Self{
        LibraryGate {
            name:         app.title.clone(),
            input_count:  app.graph.inputs.len(),
            output_count: app.graph.outputs.len(),
            graph:        app.graph.clone(),
        }
    }
}

impl App {
    // ─────────────────────────────────────────────────────────────────────────
    //  Library persistence
    // ─────────────────────────────────────────────────────────────────────────

    /// Save the current canvas as a `LibraryGate`.
    pub fn save_current_graph_to_library(&mut self) {
        let new_gate = LibraryGate::from_editor(self);

        if let Some(library_index) = self.library.iter().position(|saved| saved.name == new_gate.name) {
            self.library[library_index] = new_gate.clone();

            Self::update_saved_gate_instances_in_graph(
                &mut self.graph,
                library_index,
                &new_gate,
            );
            
            let mut library = std::mem::take(&mut self.library);
            for library_gate in &mut library {
                Self::update_saved_gate_instances_in_graph(
                    &mut library_gate.graph,
                    library_index,
                    &new_gate,
                );
            }
            self.library = library;
        } else {
            self.library.push(new_gate);
        }
    }

    /// Refresh every `SavedGate(library_index)` node inside `graph` to match
    /// the current `updated_library_gate` definition.
    pub fn update_saved_gate_instances_in_graph(
        graph: &mut EditorGraph,
        library_index: usize,
        updated_library_gate: &LibraryGate,
    ) {
        let gate_base = graph.inputs.len() + graph.outputs.len();

        let matching_node_ids: Vec<usize> = graph
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(gate_index, node)| {
                if matches!(node.kind, EditorNodeKind::SavedGate(idx) if idx == library_index) {
                    Some(gate_base + gate_index)
                } else {
                    None
                }
            })
            .collect();

        graph.wires.retain(|wire| {
            for &node_id in &matching_node_ids {
                if wire.to.node == node_id && wire.to.port >= updated_library_gate.input_count {
                    return false;
                }
                if wire.from.node == node_id && wire.from.port >= updated_library_gate.output_count {
                    return false;
                }
            }
            true
        });

        for node in graph.nodes.iter_mut() {
            if !matches!(node.kind, EditorNodeKind::SavedGate(idx) if idx == library_index) {
                continue;
            }
            node.label         = updated_library_gate.name.clone();
            node.input_count   = updated_library_gate.input_count;
            node.output_count  = updated_library_gate.output_count;
            node.input_labels  = updated_library_gate.graph.inputs.clone();
            node.output_labels = updated_library_gate.graph.outputs.clone();
        }
    }

    pub fn save_library_to_file(&mut self) {
        fn fallible_save(library: &Vec<LibraryGate>) -> Result<(), String> {
            let file = std::fs::File::create("my_library.lbl").map_err(|err| format!("Failed to open file on load {}", err.to_string()))?;
            bincode::config::DefaultOptions::new()
                .with_limit(10 * 1024 * 1024)
                .serialize_into(
                    file, &library
                )
                .map_err(|err| format!("Serialize on save failed {}", err.to_string()))
        }
        match fallible_save(&self.library) {
            Ok(_) => (),
            Err(error)  => self.simulation_error = Some(error.to_string()),
        }
    }

    pub fn load_library_from_file(&mut self) {
        fn fallible_load() -> Result<Vec<LibraryGate>, String> {
            let file = std::fs::File::open("my_library.lbl").map_err(|err| format!("Failed to open file on load {}", err.to_string()))?;
            bincode::config::DefaultOptions::new()
                .with_limit(10 * 1024 * 1024)
                .deserialize_from(file)
                .map_err(|err| format!("Deserialize on load failed {}", err.to_string()))
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
    pub fn delete_library_gate(&mut self, deleted_index: usize) {
        self.graph.remove_all_gates_of_library_index(deleted_index);

        let mut library = std::mem::take(&mut self.library);
        for library_gate in &mut library {
            library_gate.graph.remove_all_gates_of_library_index(deleted_index);
        }
        self.library = library;

        self.library.remove(deleted_index);

        self.graph.remap_saved_gate_indices_after_library_deletion(deleted_index);
        let mut library = std::mem::take(&mut self.library);
        for library_gate in &mut library {
            library_gate.graph.remap_saved_gate_indices_after_library_deletion(deleted_index);
        }
        self.library = library;

        self.library_rename_index = None;
        self.library_rename_text.clear();
    }
}