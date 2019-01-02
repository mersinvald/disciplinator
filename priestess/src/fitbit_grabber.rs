use crate::{
    ActivityGrabber, DailyActivityStats, DetailedActivityStats, HourlyActivityStats, SleepInterval,
};

use fitbit::activities::Activities;
use fitbit::sleep::Sleep;
use fitbit::{FitbitAuth, FitbitClient};

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use fitbit::date::Date;

use failure::{format_err, Error};
use log::{error, info};

use serde::Deserialize;

//use oauth2::Token as OAuthToken;
pub use fitbit::Token as FitbitToken;

pub struct FitbitActivityGrabber {
    client: FitbitClient,
    token: FitbitToken,
}

pub struct FitbitAuthData {
    pub id: String,
    pub secret: String,
    pub token: Option<FitbitToken>,
}

impl FitbitActivityGrabber {
    /// Attempt to authenticate with Firbit API. This method has 2 modes:
    /// - First auth: authenticate via OAuth2, this will open the browser in order to authenticate.
    /// - Token exists in FitbitAuthData::token: refresh the token, reopen the existing session, no user input is required
    ///   will operate as if it was the first auth attempt.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(adata: &FitbitAuthData) -> Result<Self, Error> {
        // Reopen session
        if let Some(token) = adata.token.as_ref() {
            info!("trying to authenticate with token");
            let auth = FitbitAuth::new(&adata.id, &adata.secret);
            // Refresh token to ensure one provided is valid
            if let Ok(token) = auth
                .exchange_refresh_token(token.clone())
                .map_err(|e| error!("{}", e))
            {
                info!("refresh token exchanged");
                // Convert to Fitbit Token
                let token = FitbitToken::from(token);
                // This does not send any requests, so any fail is not an auth fail
                return Ok(FitbitActivityGrabber {
                    client: FitbitClient::new(token.clone())?,
                    token,
                });
            }
        }

        info!("authenticating via OAuth2");

        // First time auth
        let auth = FitbitAuth::new(&adata.id, &adata.secret);
        let token = FitbitToken::from(auth.get_token()?);
        let client = FitbitClient::new(token.clone())?;
        Ok(FitbitActivityGrabber { client, token })
    }

    /// Return auth token
    pub fn get_token(&self) -> &FitbitToken {
        &self.token
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct FitbitActivity {
    sedentary_minutes: u32,
    lightly_active_minutes: u32,
    fairly_active_minutes: u32,
    very_active_minutes: u32,
}

use serde_json::Value;
use std::collections::HashMap;

impl ActivityGrabber for FitbitActivityGrabber {
    fn fetch_daily_activity_stats(&self, date: NaiveDate) -> Result<DailyActivityStats, Error> {
        #[derive(Deserialize)]
        struct Root {
            summary: FitbitActivity,
        }

        let response = self
            .client
            .get_daily_activity_summary("-", &Date::from(date))?;
        let Root { summary } = serde_json::from_str(&response)?;

        let activity = DailyActivityStats {
            sedentary_minutes: summary.sedentary_minutes,
            active_minutes: summary.fairly_active_minutes
                + summary.lightly_active_minutes
                + summary.very_active_minutes,
            detailed: Some(DetailedActivityStats {
                lightly_active: summary.lightly_active_minutes,
                fairly_active: summary.fairly_active_minutes,
                heavy_active: summary.very_active_minutes,
            }),
        };

        Ok(activity)
    }

    fn fetch_hourly_activity(&self, date: NaiveDate) -> Result<Vec<HourlyActivityStats>, Error> {
        // Request calories log minute-by minute: Fitbit API assigns an activity index for each entry
        let response = self
            .client
            .get_log_calories_intraday("-", &Date::from(date), "1min")?;
        let json: Value = serde_json::from_str(&response)?;
        let dataset = json
            .get("activities-log-calories-intraday")
            .and_then(|v| v.get("dataset"))
            .ok_or_else(|| format_err!("invalid json"))?;
        let time_series = parse_json_timed_values(dataset)?;

        // Collect results into the hashmap for convenience
        let mut hourly_stats = HashMap::new();
        for value in time_series {
            let stat = hourly_stats
                .entry(value.time.hour())
                .or_insert(HourlyActivityStats {
                    hour: value.time.hour(),
                    ..HourlyActivityStats::default()
                });

            let mut detailed = stat.detailed.take().unwrap_or_default();

            match value.level {
                0 => stat.sedentary_minutes += 1,
                1 => detailed.lightly_active += 1,
                2 => detailed.fairly_active += 1,
                3 => detailed.heavy_active += 1,
                e => panic!("unexpected activity level {}", e),
            }

            stat.active_minutes =
                detailed.lightly_active + detailed.fairly_active + detailed.heavy_active;
            stat.detailed = Some(detailed);
        }

        // sort entries hour-wise and collect into vector
        let mut hourly_stats = hourly_stats.drain().map(|(_k, v)| v).collect::<Vec<_>>();
        hourly_stats.sort_by_key(|v| v.hour);

        // set complete flags for finished hours
        let len = hourly_stats.len();
        if len != 0 {
            hourly_stats
                .iter_mut()
                .take(len - 1)
                .for_each(|v| v.complete = true);
        }

        Ok(hourly_stats)
    }

    fn fetch_sleep_intervals(&self, date: NaiveDate) -> Result<Vec<SleepInterval>, Error> {
        let response = self.client.get_sleep_log(&"-", &Date::from(date))?;
        let json: Value = serde_json::from_str(&response)?;

        let sleeps = json
            .get("sleep")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format_err!("invalid json: expected '{{ \"sleep\": [ ... ] }}'"))?;

        let intervals_utc = sleeps.iter().map(|v| {
            let start = v.get("startTime");
            let end = v.get("endTime");
            start.and_then(|s| end.map(|e| (s, e)))
        });

        let mut intervals = Vec::new();

        for interval in intervals_utc {
            let (start, end) = interval.ok_or_else(|| {
                format_err!("invalid json: fields 'startTime' and 'endTime' are missing")
            })?;
            let mut start: NaiveDateTime = serde_json::from_value(start.to_owned())?;
            let mut end: NaiveDateTime = serde_json::from_value(end.to_owned())?;
            // Normalize by current date
            if start.date().day() != date.day() {
                start = NaiveDateTime::new(date, NaiveTime::from_hms(0, 0, 0));
            }
            if end.date().day() != date.day() {
                end = NaiveDateTime::new(date, NaiveTime::from_hms(23, 59, 59));
            }

            intervals.push(SleepInterval {
                start: start.time(),
                end: end.time(),
            })
        }

        Ok(intervals)
    }
}

struct TimedValue {
    level: u32,
    time: NaiveTime,
}

fn parse_json_timed_values(json: &Value) -> Result<Vec<TimedValue>, Error> {
    let mut timedvalues = Vec::new();

    let array = json
        .as_array()
        .ok_or_else(|| format_err!("invalid json: expected an array"))?;
    for value in array {
        let object = value
            .as_object()
            .ok_or_else(|| format_err!("invalid json: expected an object"))?;
        let time = object
            .get("time")
            .ok_or_else(|| format_err!("missing field 'time'"))?;
        let level = object
            .get("level")
            .ok_or_else(|| format_err!("missing field 'level'"))?;
        timedvalues.push(TimedValue {
            time: serde_json::from_value(time.to_owned())?,
            level: serde_json::from_value(level.to_owned())?,
        })
    }

    Ok(timedvalues)
}

use std::fs::File;
use std::io::Write;
use std::path::Path;

pub trait TokenStore: Sized {
    fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Error>;
    fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error>;
}

impl TokenStore for FitbitToken {
    fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let json = serde_json::to_string(&self).unwrap();
        File::create(&path).and_then(|mut file| file.write_all(json.as_bytes()))?;
        Ok(())
    }

    fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let f = File::open(path)?;
        Ok(serde_json::from_reader(f)?)
    }
}
