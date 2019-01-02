mod fitbit_grabber;

pub use crate::fitbit_grabber::{FitbitActivityGrabber, FitbitAuthData, TokenStore, FitbitToken};
use failure::Error;

#[derive(Copy, Clone, Debug)]
pub struct DailyActivityStats {
    sedentary_minutes: u32,
    active_minutes: u32,
    detailed: Option<DetailedActivityStats>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct DetailedActivityStats {
    lightly_active: u32,
    fairly_active: u32,
    heavy_active: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct HourlyActivityStats {
    pub hour: u32,
    pub complete: bool,
    pub sedentary_minutes: u32,
    pub active_minutes: u32,
    pub detailed: Option<DetailedActivityStats>,
}

#[derive(Copy, Clone, Debug)]
pub struct SleepInterval {
    pub start: chrono::NaiveTime,
    pub end: chrono::NaiveTime,
}

pub trait ActivityGrabber {
    fn fetch_daily_activity_stats(&self, date: chrono::NaiveDate) -> Result<DailyActivityStats, Error>;
    fn fetch_hourly_activity(&self, date: chrono::NaiveDate) -> Result<Vec<HourlyActivityStats>, Error>;
    fn fetch_sleep_intervals(&self, date: chrono::NaiveDate) -> Result<Vec<SleepInterval>, Error>;
}


