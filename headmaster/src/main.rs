use failure::Error;
use log::{debug, error, info};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenStore,
};
use serde::{Deserialize, Serialize};

use std::env;

use chrono::{Local, NaiveDate, NaiveTime, Timelike};

mod config;
use crate::config::Config;

fn main() -> Result<(), Error> {
    dotenv::dotenv()?;

    // Load config
    let config = Config::load("headmaster.toml")?;

    // Connect to Fitbit API
    let auth_data = load_auth_data(&config)?;
    let grabber = FitbitActivityGrabber::new(&auth_data)?;

    // Save refreshed token
    let token = grabber.get_token();
    token.save(".fitbit_token")?;

    let master = Headmaster::new(grabber, config);

    let debt = master.current_hour_summary()?;

    println!("debt: {:?}", debt);

    Ok(())
}

struct Headmaster<A> {
    config: Config,
    grabber: A,
}

#[derive(Debug, Copy, Clone)]
struct Hour {
    hour: u32,
    complete: bool,
    active_minutes: u32,
    debt: u32,
}

enum State {
    Normal,
    DebtCollection(CurrentHourSummary),
    DebtCollectionPaused(CurrentHourSummary),
}

#[derive(Copy, Clone, Debug, Serialize)]
struct CurrentHourSummary {
    debt: u32,
    active_minutes: u32,
}

impl<A: ActivityGrabber> Headmaster<A> {
    pub fn new(grabber: A, config: Config) -> Self {
        Headmaster { config, grabber }
    }

    pub fn current_hour_summary(&self) -> Result<CurrentHourSummary, Error> {
        let hours = self.get_active_minutes_hourly()?;
        debug!("ABSOLUTE DEBT: \n{:#?}", hours);
        let hours = self.exclude_inactive_hours(hours)?;
        debug!("NORMALIZED BY SLEEPING HOURS: \n{:#?}", hours);
        let hours = self.normalize_by_threshold(hours);
        info!("NORMALIZED BY THRESHOLD: \n{:#?}", hours);
        let hours = self.calculate_debt_hourly(hours);
        info!("HOURLY DEBT CALCULATION: \n{:#?}", hours);
        let debt = self.calculate_debt(&hours);
        info!("CURRENT DEBT: {}", debt);
        Ok(CurrentHourSummary {
            debt,
            active_minutes: hours
                .last()
                .map(|h| h.active_minutes)
                // Failsafe to not to reach the DebtCollection state in case of error
                .unwrap_or_else(|| {
                    error!("last hour info is not availible");
                    self.config.limits.hourly_max_accounted_active_time
                }),
        })
    }

    pub fn current_state(&self) -> Result<State, Error> {
        let stat = self.current_hour_summary()?;
        let max_accounted = self.config.limits.hourly_max_accounted_active_time;
        if stat.debt > 0 && stat.active_minutes < max_accounted {
            Ok(State::DebtCollection(stat))
        } else if stat.debt > 0 && stat.active_minutes >= max_accounted {
            Ok(State::DebtCollectionPaused(stat))
        } else {
            Ok(State::Normal)
        }
    }

    fn get_active_minutes_hourly(&self) -> Result<Vec<Hour>, Error> {
        let mut data = self
            .grabber
            .fetch_hourly_activity(Self::current_date())?
            .iter()
            .map(|h| Hour {
                hour: h.hour,
                complete: h.complete,
                active_minutes: h.active_minutes,
                debt: 0,
            })
            .collect::<Vec<_>>();

        Ok(data)
    }

    fn exclude_inactive_hours(&self, mut hours: Vec<Hour>) -> Result<Vec<Hour>, Error> {
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
                // Zero debt, zero overtime
                let max_activity_during_sleep = self.config.limits.hourly_minimum_active_time;
                if h.hour >= interval.start.hour() && h.hour < interval.end.hour() {
                    h.active_minutes = max_activity_during_sleep;
                } else if h.hour == interval.end.hour() {
                    h.active_minutes = u32::min(interval.end.minute(), max_activity_during_sleep);
                }
            }
        });

        Ok(hours)
    }

    fn normalize_by_threshold(&self, mut hours: Vec<Hour>) -> Vec<Hour> {
        hours.iter_mut().for_each(|h| {
            let limits = &self.config.limits;
            h.active_minutes = u32::min(h.active_minutes, limits.hourly_max_accounted_active_time);
        });

        hours
    }

    fn calculate_debt_hourly(&self, mut hours: Vec<Hour>) -> Vec<Hour> {
        let limits = &self.config.limits;

        // Calculate first hour activity debt
        hours[0].debt = limits
            .hourly_minimum_active_time
            .checked_sub(hours[0].active_minutes)
            .unwrap_or(0);

        for i in 1..hours.len() {
            // Next hour debt is previous hour debt + current hour default debt
            let current_hour_minimum = if hours[i].complete {
                limits.hourly_minimum_active_time
            } else {
                0
            };

            hours[i].debt = (current_hour_minimum + hours[i - 1].debt)
                .checked_sub(hours[i].active_minutes)
                .unwrap_or(0)
        }

        hours
    }

    fn calculate_debt(&self, hours: &[Hour]) -> u32 {
        let limits = &self.config.limits;
        hours
            .last()
            .map(|h| u32::min(h.debt, limits.absolute_debt_limit))
            .unwrap_or(0)
    }

    fn current_date() -> NaiveDate {
        Local::today().naive_local()
    }
}

fn load_auth_data(config: &Config) -> Result<FitbitAuthData, Error> {
    let id = config.auth.client_id.clone();
    let secret = config.auth.client_secret.clone();
    let token = FitbitToken::load(".fitbit_token").ok();

    Ok(FitbitAuthData { id, secret, token })
}
