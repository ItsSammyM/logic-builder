use egui::Pos2;
use serde::{Deserialize, Serialize};

use crate::sim_builder::PortRef;

// ─────────────────────────────────────────────────────────────────────────────
//  Serde helper for egui::Pos2
// ─────────────────────────────────────────────────────────────────────────────

pub mod pos2_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(pos: &Pos2, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (pos.x, pos.y).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pos2, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (x, y) = <(f32, f32)>::deserialize(deserializer)?;
        Ok(Pos2 { x, y })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  EditorNodeKind
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum EditorNodeKind {
    Nand,
    /// Index into App::library, identifying which saved gate this instance represents.
    SavedGate(usize),
}

// ─────────────────────────────────────────────────────────────────────────────
//  EditorNode — one gate placed on the canvas
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditorNode {
    pub label: String,
    /// Top-left corner in canvas space (not screen space).
    #[serde(with = "pos2_serde")]
    pub pos: Pos2,
    pub input_count: usize,
    pub output_count: usize,
    pub kind: EditorNodeKind,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Wire — a directed connection from one output port to one input port
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Wire {
    pub from: PortRef,
    pub to: PortRef,
}

// ─────────────────────────────────────────────────────────────────────────────
//  LibraryGate — a saved circuit that can be re-used as a sub-gate
// ─────────────────────────────────────────────────────────────────────────────

/// A gate that has been saved to the library.
/// Stores both display metadata and the complete editor graph so it can be re-opened.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LibraryGate {
    pub name: String,
    pub input_count: usize,
    pub output_count: usize,
    pub graph: EditorGraph,
}

// ─────────────────────────────────────────────────────────────────────────────
//  EditorGraph — the full graph for one circuit level
// ─────────────────────────────────────────────────────────────────────────────

/// The editor's full representation of one circuit level.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EditorGraph {
    /// Names of the external input ports shown on the left rail.
    pub inputs: Vec<String>,
    /// Names of the external output ports shown on the right rail.
    pub outputs: Vec<String>,
    /// Internal gate nodes placed on the canvas.
    pub nodes: Vec<EditorNode>,
    /// All wires connecting ports to each other.
    pub wires: Vec<Wire>,
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

    pub fn input_node_id(&self, input_index: usize) -> usize {
        input_index
    }

    pub fn output_node_id(&self, output_index: usize) -> usize {
        self.inputs.len() + output_index
    }

    pub fn gate_node_id(&self, gate_index: usize) -> usize {
        self.inputs.len() + self.outputs.len() + gate_index
    }

    pub fn is_input_node(&self, node_id: usize) -> bool {
        node_id < self.inputs.len()
    }

    pub fn is_output_node(&self, node_id: usize) -> bool {
        node_id >= self.inputs.len() && node_id < self.inputs.len() + self.outputs.len()
    }

    /// Converts a flat node_id back to an index into `self.nodes`, if it refers to a gate.
    pub fn gate_index_from_node_id(&self, node_id: usize) -> Option<usize> {
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
    pub fn remove_input(&mut self, input_index: usize) {
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
    pub fn remove_output(&mut self, output_index: usize) {
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
    pub fn remove_gate(&mut self, gate_index: usize) {
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
    pub fn remove_wire(&mut self, wire_to_remove: &Wire) {
        self.wires.retain(|wire| wire != wire_to_remove);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  BulkWireState — state machine for the Shift+drag box-select bulk-wiring feature
// ─────────────────────────────────────────────────────────────────────────────

/// Which phase of the box-select bulk-wire operation we are in.
#[derive(Clone, Debug, Default)]
pub enum BulkWireState {
    /// Nothing happening.
    #[default]
    Idle,
    /// User is dragging a selection box to choose output ports (phase 1).
    SelectingOutputs {
        drag_start_canvas: Pos2,
        drag_current_canvas: Pos2,
    },
    /// Phase 1 complete — outputs chosen, waiting for the user to Shift+drag to select inputs.
    OutputsChosen {
        /// The output PortRefs collected in phase 1, in top-to-bottom order.
        selected_output_ports: Vec<PortRef>,
    },
    /// User is dragging a selection box to choose input ports (phase 2).
    SelectingInputs {
        selected_output_ports: Vec<PortRef>,
        drag_start_canvas: Pos2,
        drag_current_canvas: Pos2,
    },
}
