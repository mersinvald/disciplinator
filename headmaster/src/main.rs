use failure::Error;
use log::{debug, info};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenStore,
};
use serde::{Deserialize, Serialize};

use std::env;

use chrono::{Local, NaiveDate, NaiveTime, Timelike};

fn main() -> Result<(), Error> {
    dotenv::dotenv()?;
    env_logger::init();

    // Connect to Fitbit API
    let auth_data = load_auth_data()?;
    let grabber = FitbitActivityGrabber::new(&auth_data)?;

    let token = grabber.get_token();
    token.save(".fitbit_token")?;

    let master = Headmaster::new(
        grabber,
        Config {
            limits: Limits {
                hour_minimum_active_time: 5,
                hour_overtime_limit: 2,
                absolute_debt_limit: 15,
                absolute_overtime_limit: 5,
            },
            day: Day {
                day_begins_at: NaiveTime::from_hms(10, 0, 0),
                day_ends_at: NaiveTime::from_hms(20, 0, 0),
            },
        },
    );

    let debt = master.current_debt()?;

    println!("debt: {}", debt);

    Ok(())
}

struct Headmaster<A> {
    config: Config,
    grabber: A,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    limits: Limits,
    day: Day,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Limits {
    hour_minimum_active_time: i64,
    hour_overtime_limit: i64,
    absolute_debt_limit: i64,
    absolute_overtime_limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Day {
    /// used when there's no sleep data
    day_begins_at: NaiveTime,
    /// used regardless of sleep data: there should be some time for leisure in the evening
    day_ends_at: NaiveTime,
}

#[derive(Debug, Copy, Clone)]
struct HourWithDebt {
    hour: u32,
    complete: bool,
    debt: i64,
}

enum State {
    Normal,
    DebtCollection(i64),
}

impl<A: ActivityGrabber> Headmaster<A> {
    pub fn new(grabber: A, config: Config) -> Self {
        Headmaster { config, grabber }
    }

    pub fn current_debt(&self) -> Result<i64, Error> {
        let hours = self.get_absolute_debt_hourly()?;
        debug!("ABSOLUTE DEBT: \n{:#?}", hours);
        let hours = self.exclude_inactive_hours(hours)?;
        debug!("NORMALIZED BY SLEEPING HOURS: \n{:#?}", hours);
        let hours = self.normalize_by_hourly_threshold(hours);
        info!("NORMALIZED BY HOURLY THRESHOLD: \n{:#?}", hours);
        let debt = self.calculate_debt(&hours);
        info!("CURRENT DEBT: {}", debt);
        Ok(debt)
    }

    pub fn current_state(&self) -> Result<State, Error> {
        let debt = self.current_debt()?;
        if debt > 0 {
            Ok(State::DebtCollection(debt))
        } else {
            Ok(State::Normal)
        }
    }

    fn get_absolute_debt_hourly(&self) -> Result<Vec<HourWithDebt>, Error> {
        let data = self
            .grabber
            .fetch_hourly_activity(Self::current_date())?
            .iter()
            .map(|h| HourWithDebt {
                hour: h.hour,
                complete: h.complete,
                debt: self.config.limits.hour_minimum_active_time - i64::from(h.active_minutes),
            })
            .collect();

        Ok(data)
    }

    fn exclude_inactive_hours(
        &self,
        mut hours: Vec<HourWithDebt>,
    ) -> Result<Vec<HourWithDebt>, Error> {
        // Fetch the sleeping intervals from FitBit API
        let mut sleep_intervals = self.grabber.fetch_sleep_intervals(Self::current_date())?;

        // If no data there, fallback to config defined day start time
        if sleep_intervals.is_empty() {
            sleep_intervals.push(SleepInterval {
                start: NaiveTime::from_hms(0, 0, 0),
                end: self.config.day.day_begins_at,
            })
        }

        // Add the day end interval as well
        sleep_intervals.push(SleepInterval {
            start: self.config.day.day_ends_at,
            end: NaiveTime::from_hms(23, 59, 59),
        });

        hours.iter_mut().for_each(|h| {
            for interval in &sleep_intervals {
                if h.hour >= interval.start.hour() && h.hour < interval.end.hour() {
                    h.debt = 0;
                } else if h.hour == interval.end.hour() {
                    h.debt -= i64::from(interval.end.minute());
                    if h.debt < 0 {
                        h.debt = 0
                    }
                }
            }
        });

        Ok(hours)
    }

    fn normalize_by_hourly_threshold(&self, mut hours: Vec<HourWithDebt>) -> Vec<HourWithDebt> {
        hours.iter_mut().for_each(|h| {
            let limits = &self.config.limits;
            if h.debt > limits.hour_minimum_active_time {
                h.debt = limits.hour_minimum_active_time;
            }

            if h.debt < (0 - limits.hour_overtime_limit) {
                h.debt = 0 - limits.hour_overtime_limit
            }
        });

        hours
    }

    fn calculate_debt(&self, hours: &[HourWithDebt]) -> i64 {
        let limits = &self.config.limits;
        // Moving threshold to handle the cases when
        // hour 1: debt -60
        // and N consecutive hours are debt-free
        hours
            .iter()
            .filter(|h| h.complete)
            .map(|h| h.debt)
            .fold(0, |mut acc, debt| {
                acc += debt;
                if acc > limits.absolute_debt_limit {
                    acc = limits.absolute_debt_limit
                }
                if acc < (0 - limits.absolute_overtime_limit) {
                    acc = 0 - limits.absolute_overtime_limit
                }
                acc
            })
    }

    fn current_date() -> NaiveDate {
        Local::today().naive_local()
    }
}

fn load_auth_data() -> Result<FitbitAuthData, Error> {
    let id = env::var("FITBIT_CLIENT_ID")?;
    let secret = env::var("FITBIT_CLIENT_SECRET")?;
    let token = FitbitToken::load(".fitbit_token").ok();

    Ok(FitbitAuthData { id, secret, token })
}
