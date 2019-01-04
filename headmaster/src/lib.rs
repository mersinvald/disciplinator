use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct CurrentHourSummary {
    pub debt: u32,
    pub active_minutes: u32,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum State {
    Normal(CurrentHourSummary),
    DebtCollection(CurrentHourSummary),
    DebtCollectionPaused(CurrentHourSummary),
}

impl State {
    pub fn is_debt_collection(self) -> bool {
        match self {
            State::DebtCollection(..) => true,
            _ => false,
        }
    }
}
