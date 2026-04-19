use super::prelude::*;

pub type SingleGateIoId = u16;



#[derive(Debug, Clone, Hash, Copy, PartialEq, Eq)]
pub(crate) struct GateInput(GateId, SingleGateIoId);
impl GateInput{
    pub unsafe fn new_unchecked(parent: &SimulationBox, gate: GateId, slot: SingleGateIoId)->Self{
        Self(gate, slot)
    }
    pub(crate) fn new_from_gate(parent: &SimulationBox, gate: GateId, slot: SingleGateIoId)->Option<Self>{
        if slot < gate.get_gate(parent).num_outputs() {
            Some(Self(gate, slot))
        }else{
            None
        }
    }
}
#[derive(Debug, Clone, Hash, Copy, PartialEq, Eq)]
pub(crate) struct GateOutput(GateId, SingleGateIoId);
impl GateOutput{
    pub unsafe fn new_unchecked(parent: &SimulationBox, gate: GateId, slot: SingleGateIoId)->Self{
        Self(gate, slot)
    }
    pub(crate) fn new_from_gate(parent: &SimulationBox, gate: GateId, slot: SingleGateIoId)->Option<Self>{
        if slot < gate.get_gate(parent).num_outputs() {
            Some(Self(gate, slot))
        }else{
            None
        }
    }
    pub(super) fn get_state(&self, parent: &SimulationBox)->bool{
        unsafe{self.0.get_state(parent).get_unchecked(self.1)}
    }
}