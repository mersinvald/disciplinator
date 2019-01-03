use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct CurrentHourSummary {
    pub debt: u32,
    pub active_minutes: u32,
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub enum State {
    Normal,
    DebtCollection(CurrentHourSummary),
    DebtCollectionPaused(CurrentHourSummary),
}
