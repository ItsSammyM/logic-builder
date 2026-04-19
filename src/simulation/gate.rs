use enum_delegate;
use super::prelude::*;

#[enum_delegate::implement(GateTrait)]
#[derive(Clone)]
pub(crate) enum Gate{
    Nand(NandGate),
    Const(ConstGate),
    // CompositeBoxGate(CompositeBoxGate),
}

#[enum_delegate::register]
pub(super) trait GateTrait{
    fn generate_state(&self, id: GateId, parent: &SimulationBox) -> GateState;
    fn num_outputs(&self) -> u16;
    fn num_inputs(&self) -> u16;
}



#[derive(Clone)]
pub(crate) struct NandGate;
impl NandGate{
    pub(crate) fn new()->Gate{
        Gate::Nand(Self)
    }
}
impl GateTrait for NandGate{
    fn generate_state(&self, id: GateId, parent: &SimulationBox) -> GateState {

        let state = match (
            parent.wires.get(&unsafe{GateInput::new_unchecked(parent, id, 0)}),
            parent.wires.get(&unsafe{GateInput::new_unchecked(parent, id, 1)})
        ) {
            (None, None) => false,
            (None, Some(_)) => false,
            (Some(_), None) => false,
            (Some(a), Some(b)) => !(a.get_state(parent) && b.get_state(parent)),
        };
        vec![state].into()
    }
    fn num_inputs(&self) -> u16 {
        2
    }
    fn num_outputs(&self) -> u16 {
        1   
    }
}

#[derive(Clone, Default)]
pub(crate) struct ConstGate(pub(super) GateState);
impl ConstGate {
    pub(crate) fn new_true()->Gate{
        Gate::Const(ConstGate(vec![true].into()))
    }
    pub(crate) fn new_false()->Gate{
        Gate::Const(ConstGate(vec![false].into()))
    }
    pub(crate) fn new_from_state(state: GateState)->Gate{
        Gate::Const(ConstGate(state))
    }
}
impl GateTrait for ConstGate{
    fn generate_state(&self, _id: GateId, _parent: &SimulationBox) -> GateState {
        self.0.clone().into()
    }
    fn num_inputs(&self) -> SingleGateIoId {
        0
    }
    fn num_outputs(&self) -> SingleGateIoId {
        self.0.len()
    }
}