use egui::{Pos2, Rect, pos2};

use crate::sim_builder::PortRef;

use super::app::App;
use super::constants::IO_RAIL_STEP;
use super::geometry::{input_port_canvas_pos, output_port_canvas_pos};

impl App {
    // ─────────────────────────────────────────────────────────────────────────
    //  Coordinate transforms
    // ─────────────────────────────────────────────────────────────────────────

    /// Convert a position from canvas space to screen (pixel) space.
    pub fn canvas_to_screen(&self, canvas_pos: Pos2, canvas_origin: Pos2) -> Pos2 {
        canvas_origin + (canvas_pos.to_vec2() + self.canvas_pan) * self.canvas_zoom
    }

    /// Convert a position from screen (pixel) space to canvas space.
    pub fn screen_to_canvas(&self, screen_pos: Pos2, canvas_origin: Pos2) -> Pos2 {
        ((screen_pos - canvas_origin) / self.canvas_zoom - self.canvas_pan).to_pos2()
    }

    // ─────────────────────────────────────────────────────────────────────────
    //  Port → screen position
    // ─────────────────────────────────────────────────────────────────────────

    /// Return the screen-space position of a port dot, or `None` if the port
    /// reference is out of range.
    ///
    /// `is_output_port` is only consulted for gate nodes (I/O rail ports are
    /// always on fixed sides of the canvas).
    pub fn port_to_screen_pos(
        &self,
        port: &PortRef,
        is_output_port: bool,
        canvas_origin: Pos2,
        canvas_rect: Rect,
    ) -> Option<Pos2> {
        let input_count  = self.graph.inputs.len();
        let output_count = self.graph.outputs.len();
        let center_y = canvas_rect.center().y;

        // Input pseudo-nodes live on the left rail.
        if self.graph.is_input_node(port.node) {
            let start_y = center_y - (input_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            return Some(pos2(
                canvas_rect.left() + 14.0,
                start_y + port.node as f32 * IO_RAIL_STEP,
            ));
        }

        // Output pseudo-nodes live on the right rail.
        if self.graph.is_output_node(port.node) {
            let output_index = port.node - input_count;
            let start_y = center_y - (output_count as f32 - 1.0) * IO_RAIL_STEP / 2.0;
            return Some(pos2(
                canvas_rect.right() - 14.0,
                start_y + output_index as f32 * IO_RAIL_STEP,
            ));
        }

        // Internal gate nodes: convert the canvas-space port position to screen.
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
}
