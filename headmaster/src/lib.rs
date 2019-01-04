use serde::{Deserialize, Serialize};

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub state: State,
    pub day_log: Vec<HourSummary>,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourSummary {
    pub hour: u32,
    pub debt: u32,
    pub active_minutes: u32,
    pub tracking_disabled: bool,
    pub complete: bool,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum State {
    Normal(HourSummary),
    DebtCollection(HourSummary),
    DebtCollectionPaused(HourSummary),
}

impl State {
    pub fn is_debt_collection(self) -> bool {
        match self {
            State::DebtCollection(..) => true,
            _ => false,
        }
    }
}
