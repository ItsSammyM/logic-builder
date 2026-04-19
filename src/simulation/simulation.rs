use std::collections::HashMap;

use super::prelude::*;

#[derive(Clone)]
pub(crate) struct SimulationBox{
    //inputs are represented using GateId 0 as a const gate.


    pub(super)gates: HashMap<GateId, Gate>,
    pub(super)state: HashMap<GateId, GateState>,

    //wires can split but not merge IMPLIES
    //Each input can only be associated with 1 output IMPLIES
    //This map goes backwards from GateInput -> GateOutput. 
    pub(super) wires: HashMap<GateInput, GateOutput>,

    pub(super)next_id: GateId,

    pub(super) num_outputs: u16,
    pub(super) outputs: Vec<GateOutput>,
}
impl SimulationBox{
    pub(super) fn next_id(&mut self)->GateId{
        let id = self.next_id;
        self.next_id = self.next_id.next();
        id
    }
    pub(crate) fn next_step(&mut self){
        let old = self.clone();
        for (id, gate) in &old.gates {
            *id.get_state_mut(self) = gate.generate_state(*id, &old);
        }
    }
    pub(crate) fn get_output_state(&self) -> GateState {
        let out: Vec<_> = self.outputs.iter().map(|w|w.get_state(self)).collect();
        out.into()
    }
    
    pub(crate) fn new_empty()->Self{
        let mut out = Self {
            gates: HashMap::new(),
            state: HashMap::new(),
            next_id: GateId::input_sentinel(),
            wires: HashMap::new(),
            num_outputs: 0,
            outputs: vec![]
        };
        out.insert_gate(Gate::Const(ConstGate::default()));
        out
    }
    pub(crate) fn insert_gate(&mut self, gate: Gate) -> GateId {
        let state = GateState::new_from_gate(&gate);
        self.insert_gate_with_state(gate, state)
    }
    pub(crate) fn replace_gate(&mut self, id: GateId, gate: Gate, mut state: GateState) {
        state.set_to_gate_size(&gate);
        self.state.insert(id, state);
        self.gates.insert(id, gate);
    }
    pub(crate) fn insert_wire(&mut self, start: GateOutput, end: GateInput) {
        self.wires.insert(end, start);
    }
    pub(crate) fn insert_gate_with_state(&mut self, gate: Gate, mut state: GateState) -> GateId {
        let id = self.next_id();
        state.set_to_gate_size(&gate);
        self.state.insert(id, state);
        self.gates.insert(id, gate);
        id
    }
    pub(crate) fn insert_input(
        &mut self,
        default: bool
    )->GateOutput{
        let Gate::Const(ConstGate(mut const_inner)) = GateId::input_sentinel().get_gate(self).clone() else {unreachable!()};
        const_inner.push(default);
        let slot = const_inner.len() - 1;
        self.replace_gate(GateId::input_sentinel(), ConstGate::new_from_state(const_inner.clone()), const_inner);
        unsafe{GateOutput::new_unchecked(self, GateId::input_sentinel(), slot)}
    }
    pub(crate) fn insert_output(
        &mut self,
        gate_output: GateOutput
    ){
        self.num_outputs += 1;
        self.outputs.push(gate_output);
    }

    pub(crate) fn insert_simulation(&mut self, other: SimulationBox){
        //remap others gate ids to be valid in self by creating a map.
        let gate_id_map: HashMap<_, _> = other.gates.iter().map(|(id,_)|(id, self.next_id.next())).collect()
        //insert others gates
        //insert others state

        
    }
}


#[derive(Debug, Clone, Default)]
pub(crate) struct GateState{
    state: Vec<bool>
}
impl GateState{
    pub(super) unsafe fn get_unchecked(&self, id: SingleGateIoId)->bool{
        unsafe{*self.state.get_unchecked(id as usize)}
    }
    pub(super) fn new_empty()->Self{
        Self { state: vec![] }
    }
    pub(super) fn new_from_gate(gate: &Gate)->Self{
        Self { state: (0..gate.num_outputs()).map(|_|false).collect() }
    }
    pub fn set_to_gate_size(&mut self, gate: &Gate){
        self.state.resize(gate.num_outputs().into(), false);
    }
    pub fn len(&self)->SingleGateIoId{
        self.state.len().try_into().unwrap()
    }
    pub(super) fn push(&mut self, value: bool){
        self.state.push(value);
    }
}

impl From<Vec<bool>> for GateState{
    fn from(value: Vec<bool>) -> Self {
        GateState { state: value }
    }
}