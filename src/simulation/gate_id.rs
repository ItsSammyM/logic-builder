use super::prelude::*;

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) struct GateId(u16);
impl GateId{
    pub(super) fn input_sentinel()->GateId{
        Self(0)
    }
    pub(super) fn get_gate<'a>(&self, parent: &'a SimulationBox)->&'a Gate{
        parent.gates.get(self).expect("GateId must be valid")
    }
    pub(super) fn get_gate_mut<'a>(&self, parent: &'a mut SimulationBox)->&'a mut Gate{
        parent.gates.get_mut(self).expect("GateId must be valid")
    }
    pub(super) fn get_state<'a>(&self, parent: &'a SimulationBox)->&'a GateState{
        parent.state.get(self).expect("GateId must be valid")
    }
    pub(super) fn get_state_mut<'a>(&self, parent: &'a mut SimulationBox)->&'a mut GateState{
        parent.state.get_mut(self).expect("GateId must be valid")
    }
    pub(super) fn next(&self)->GateId{
        GateId(self.0 + 1)
    }

    pub(crate) fn outputs(&self, parent: &SimulationBox)->impl Iterator<Item = GateOutput>{
        (0..self.get_gate(parent).num_outputs()).map(|slot|unsafe {GateOutput::new_unchecked(parent, *self, slot)})
    }
    pub(crate) fn inputs(&self, parent: &SimulationBox)->impl Iterator<Item = GateInput>{
        (0..self.get_gate(parent).num_inputs()).map(|slot|unsafe {GateInput::new_unchecked(parent, *self, slot)})
    }
}