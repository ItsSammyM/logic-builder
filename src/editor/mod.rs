// ─────────────────────────────────────────────────────────────────────────────
//  Editor module
//
//  File layout:
//    graph.rs             — EditorGraph, EditorNode, EditorNodeKind, Wire,
//                           LibraryGate, BulkWireState, pos2_serde
//    constants.rs         — COLOR_* and layout constants (NODE_WIDTH, PORT_RADIUS, …)
//    geometry.rs          — Pure geometry: port positions, node height, snap, bezier
//    app.rs               — App struct + Default
//    coords.rs            — canvas↔screen coordinate conversions, port_to_screen_pos
//    hit_test.rs          — hit_test_port / hit_test_gate / hit_test_wire
//    draw.rs              — draw_grid, draw_io_rails, draw_wires, draw_gate_node,
//                           live_port_color_for_input / _output
//    canvas.rs            — show_canvas, update_bulk_wire, collect_ports_in_canvas_rect,
//                           draw_bulk_wire_overlay
//    panels.rs            — show_top_panel, show_left_panel, show_right_panel
//    simulation_bridge.rs — build_simulation_from_graph, step_simulation,
//                           editor_graph_to_desc, build_port_to_wire_index_map
//    actions.rs           — save/load/clear/open/delete library and canvas actions
// ─────────────────────────────────────────────────────────────────────────────

pub mod graph;
pub mod constants;
pub mod geometry;
pub mod app;
pub mod coords;
pub mod hit_test;
pub mod draw;
pub mod canvas;
pub mod panels;
pub mod simulation_bridge;
pub mod actions;

pub use app::App;
