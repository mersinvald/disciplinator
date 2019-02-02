use crate::proto::DataResponse;
use serde::{Deserialize, Serialize};

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub status: Status,
    pub day_log: Vec<HourSummary>,
}

impl DataResponse for Summary {}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Status {
    Normal(HourSummary),
    DebtCollection(HourSummary),
    DebtCollectionPaused(HourSummary),
}

impl DataResponse for Status {}

impl Status {
    #[allow(dead_code)]
    pub fn is_debt_collection(self) -> bool {
        match self {
            Status::DebtCollection(..) => true,
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
