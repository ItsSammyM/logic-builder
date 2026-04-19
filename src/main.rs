use crate::simulation::{gate::{ConstGate, Gate, NandGate}, simulation::SimulationBox};

mod simulation;

fn main() {
    let mut not_sim = SimulationBox::new_empty();
    let ext_in = not_sim.insert_input(true);
    let nand = not_sim.insert_gate(NandGate::new());
    let mut nand_inp = nand.inputs(&not_sim);
    let nand_in_a = nand_inp.next().unwrap();
    let nand_in_b = nand_inp.next().unwrap();
    drop(nand_inp);
    not_sim.insert_wire(ext_in, nand_in_a);
    not_sim.insert_wire(ext_in, nand_in_b);
    let nand_out = nand.outputs(&not_sim).next().unwrap();
    not_sim.insert_output(nand_out);

    println!("RUNNING SIM");

    for _ in 0..5{
        println!("{:?}", not_sim.get_output_state());
        not_sim.next_step();
    }

    
}






// #[derive(Clone)]
// struct CompositeBoxGate{
//     composite_box: CompositeBox,
//     input_gates: Vec<WireStart>
// }
// impl ComputeGateState for CompositeBoxGate{
//     fn next_step(&self, parent: &CompositeBox) -> (Gate, GateState) {
//         let mut new = self.clone();
//         new.composite_box.current_input = new.input_gates.iter().map(|w|w.get_state(parent)).collect();
//         new.composite_box.next_step_mut();
//         let state = new.composite_box.get_state();
//         (Gate::CompositeBoxGate(new), state)
//     }
// }