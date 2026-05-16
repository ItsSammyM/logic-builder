use serde::{Deserialize, Serialize};

use self::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireId(pub u32);
impl WireId{
    fn next_value(&self, state: &WireState)->bool{
        state.get_next(*self)
    }
    pub fn current_value(&self, state: &WireState)->bool{
        state.get_current(*self)
    }
    fn set_next(&self, state: &mut WireState, value: bool) {
        state.set_in_next(*self, value)
    }
    fn set_current(&self, state: &mut WireState, value: bool) {
        state.set_in_current(*self, value)
    }
}

pub mod simulation;
pub mod node;
pub mod wire_state;
#[cfg(test)]
mod test_gates;
pub mod prelude;