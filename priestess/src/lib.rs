mod fitbit_grabber;

use serde::{Serialize, Deserialize};
pub use crate::fitbit_grabber::{FitbitActivityGrabber, FitbitAuthData, FitbitToken, TokenJson};
use failure::{Fail, Error};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct DailyActivityStats {
    pub sedentary_minutes: u32,
    pub active_minutes: u32,
    pub detailed: Option<DetailedActivityStats>,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct DetailedActivityStats {
    pub lightly_active: u32,
    pub fairly_active: u32,
    pub heavy_active: u32,
}

#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize)]
pub struct HourlyActivityStats {
    pub hour: u32,
    pub complete: bool,
    pub sedentary_minutes: u32,
    pub active_minutes: u32,
    pub detailed: Option<DetailedActivityStats>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct SleepInterval {
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
}

pub trait ActivityGrabber: Sized {
    type AuthData;
    type Token: Clone + Sized;
    fn new(auth: &Self::AuthData) -> Result<Self, Error>;
    fn get_token(&self) -> &Self::Token;
    fn fetch_daily_activity_stats(
        &self,
        date: chrono::NaiveDate,
    ) -> Result<DailyActivityStats, Error>;
    fn fetch_hourly_activity(
        &self,
        date: chrono::NaiveDate,
    ) -> Result<Vec<HourlyActivityStats>, Error>;
    fn fetch_sleep_intervals(&self, date: chrono::NaiveDate) -> Result<Vec<SleepInterval>, Error>;
}

#[derive(Debug, Copy, Clone, Fail)]
pub enum ActivityGrabberError {
    #[fail(display = "need a new token")]
    NeedNewToken,
}
