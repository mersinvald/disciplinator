use serde::{Deserialize, Serialize};
use crate::proto::DataResponse;

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub state: State,
    pub day_log: Vec<HourSummary>,
}

impl DataResponse for Summary {}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum State {
    Normal(HourSummary),
    DebtCollection(HourSummary),
    DebtCollectionPaused(HourSummary),
}

impl DataResponse for State {}

impl State {
    pub fn is_debt_collection(self) -> bool {
        match self {
            State::DebtCollection(..) => true,
            _ => false,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourSummary {
    pub hour: u32,
    pub debt: u32,
    pub active_minutes: u32,
    #[serde(skip)]
    pub accounted_active_minutes: u32,
    pub tracking_disabled: bool,
    pub complete: bool,
}

