use priestess::{FitbitActivityGrabber, FitbitAuthData, ActivityGrabber, SleepInterval, ActivityGrabberError};
use chrono::NaiveDateTime;
use chrono::{NaiveTime, NaiveDate, Timelike};
use failure::Error;
use log::{info, debug, error};
use std::marker::PhantomData;

use crate::proto::activity::{Summary, HourSummary, State};

use actix_web::actix::{Message, Actor, SyncContext, Handler};

pub struct HeadmasterExecutor;

impl Actor for HeadmasterExecutor {
    type Context = SyncContext<Self>;
}

pub struct GetSummary<G: ActivityGrabber> {
    config: HeadmasterConfig,
    auth: G::AuthData,
}

impl<G: ActivityGrabber> GetSummary<G> {
    pub fn new(config: HeadmasterConfig, auth: G::AuthData) -> Self {
        GetSummary {
            config,
            auth,
        }
    }
}

impl<G: ActivityGrabber> Message for GetSummary<G>
    where G::Token: 'static
{
    type Result = Result<(Summary, G::Token), Error>;
}

impl<G: ActivityGrabber> Handler<GetSummary<G>> for HeadmasterExecutor
    where G::Token: 'static
{
    type Result = Result<(Summary, G::Token), Error>;

    fn handle(&mut self, msg: GetSummary<G>, _: &mut Self::Context) -> Self::Result {
        let headmaster = Headmaster::new(msg.config, msg.auth);
        let worker = headmaster.login()?;
        // WTF RUST CAN'T INFER FUCKING TYPE WITH
        // worker.get_token().clone()
        let token = <HeadmasterWorker<G>>::get_token(&worker).clone();
        let summary = worker.current_summary()?;
        Ok((summary, token))
    }
}

#[derive(Copy, Clone, Debug)]
pub struct HeadmasterConfig {
    pub minimum_active_time: i32,
    pub max_accounted_active_minutes: i32,
    pub debt_limit: i32,
    pub day_begins_at: NaiveTime,
    pub day_ends_at: NaiveTime,
    pub day_length: i32,
    pub user_date_time: NaiveDateTime,
}

pub struct Headmaster<G: ActivityGrabber> {
    auth: G::AuthData,
    config: HeadmasterConfig,
}

impl<G: ActivityGrabber> Headmaster<G> {
    pub fn new(config: HeadmasterConfig, auth: G::AuthData) -> Self {
        Headmaster {
            auth,
            config,
        }
    }

    pub fn login(self) -> Result<HeadmasterWorker<G>, Error> {
        info!("logging into FitBit API");
        let grabber = G::new(&self.auth)
            // Convert NewNewToken error into TokenExpired error, so it would be handled correctly by webserver
            .map_err(|e| {
                match e.downcast::<ActivityGrabberError>() {
                    Ok(age) => match age {
                        ActivityGrabberError::NeedNewToken => crate::proto::Error::TokenExpired.into(),
                        err => err.into()
                    },
                    Err(err) => err,
                }
            })?;

        Ok(HeadmasterWorker {
            config: self.config,
            grabber,
        })
    }
}

pub struct HeadmasterWorker<G: ActivityGrabber> {
    config: HeadmasterConfig,
    grabber: G,
}

static NOT_LOGGED_IN_PANIC_MSG: &str =
    "FitbitGrabber not logged into FirBit API. Login should be performed before any request.";

impl<G: ActivityGrabber> HeadmasterWorker<G> {
    pub fn get_token(&self) -> &G::Token {
        self.grabber.get_token()
    }

    pub fn current_hour_and_day_log(&self) -> Result<(HourSummary, Vec<HourSummary>), Error> {
        info!("logged in succesfully");
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

        let last_hour = hours.last().cloned().unwrap_or_else(|| {
            error!("last hour info is not available");
            HourSummary {
                complete: true,
                tracking_disabled: true,
                ..Default::default()
            }
        });

        Ok((last_hour, hours))
    }

    pub fn current_summary(&self) -> Result<Summary, Error> {
        // Get last stats from Fitbit
        let (hour, day_log) = self.current_hour_and_day_log()?;

        // Calculate the correct system state:
        // 1. debt > 0 and user haven't been active >= max hourly accounted time => DebtCollection
        // 2. debt > 0 and user can't log more time this hour due to the limit => DebtCollectionPaused
        // 3. no debt => Normal
        let max_accounted = self.config.max_accounted_active_minutes;
        let state = if hour.debt > 0 && hour.active_minutes < max_accounted {
            State::DebtCollection(hour)
        } else if hour.debt > 0 && hour.active_minutes >= max_accounted {
            State::DebtCollectionPaused(hour)
        } else {
            State::Normal(hour)
        };

        let summary = Summary { state, day_log };

        Ok(summary)
    }

    fn get_active_minutes_hourly(&self) -> Result<Vec<HourSummary>, Error> {
        let data = self
            .grabber
            .fetch_hourly_activity(self.current_date())?
            .iter()
            .map(|h| HourSummary {
                hour: h.hour,
                complete: h.complete,
                active_minutes: h.active_minutes,
                accounted_active_minutes: h.active_minutes,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        Ok(data)
    }

    fn exclude_inactive_hours(&self, mut hours: Vec<HourSummary>) -> Result<Vec<HourSummary>, Error> {
        // Fetch the sleeping intervals from FitBit API
        let mut sleep_intervals = self
            .grabber
            .fetch_sleep_intervals(self.current_date())?;

        debug!("sleep intervals: {:#?}", sleep_intervals);

        // If no data there, fallback to config defined day start time
        if sleep_intervals.is_empty() {
            sleep_intervals.push(SleepInterval {
                start: NaiveTime::from_hms(0, 0, 0),
                end: self.config.day_begins_at,
            })
        }

        // Calculate day end
        let day_end = sleep_intervals.iter().fold(None, |day_end, interval| {
            let end = if day_end.is_none() {
                Some(interval.end + chrono::Duration::hours(self.config.day_length as i64))
            } else {
                day_end.map(|time| time + (interval.end - interval.start))
            };

            // Check that we're not overflowing the 24-h boundary
            end.map(|e| {
                if e < interval.end {
                    NaiveTime::from_hms(23, 59, 59)
                } else {
                    e
                }
            })
        });

        debug!("day ends at: {:?}", day_end);

        // Add the day end interval as well,
        sleep_intervals.push(SleepInterval {
            start: day_end.unwrap_or(self.config.day_ends_at),
            end: NaiveTime::from_hms(23, 59, 59),
        });

        hours.iter_mut().for_each(|h| {
            for interval in &sleep_intervals {
                // Zero debt, zero overtime
                let activity_during_sleep = self.config.minimum_active_time;
                if h.hour >= interval.start.hour() as i32 && h.hour < interval.end.hour() as i32 {
                    h.accounted_active_minutes = activity_during_sleep;
                    h.tracking_disabled = true;
                } else if h.hour == interval.end.hour() as i32 {
                    h.accounted_active_minutes =
                        i32::min(interval.end.minute() as i32, activity_during_sleep);
                    if h.accounted_active_minutes == activity_during_sleep {
                        h.tracking_disabled = true;
                    }
                }
            }
        });

        Ok(hours)
    }

    fn normalize_by_threshold(&self, mut hours: Vec<HourSummary>) -> Vec<HourSummary> {
        hours.iter_mut().for_each(|h| {
            h.accounted_active_minutes =
                i32::min(h.accounted_active_minutes, self.config.max_accounted_active_minutes);
            h.debt = i32::min(h.debt, self.config.debt_limit);
        });

        hours
    }

    fn calculate_debt_hourly(&self, mut hours: Vec<HourSummary>) -> Vec<HourSummary> {
        // Calculate first hour activity debt
        hours[0].debt = self.config
            .minimum_active_time
            .checked_sub(hours[0].accounted_active_minutes)
            .unwrap_or(0);

        for i in 1..hours.len() {
            // Next hour debt is previous hour debt + current hour default debt
            let current_hour_minimum = if hours[i].complete {
                self.config.minimum_active_time
            } else {
                0
            };

            hours[i].debt = (current_hour_minimum + hours[i - 1].debt)
                .checked_sub(hours[i].accounted_active_minutes)
                .unwrap_or(0)
        }

        hours
    }

    fn calculate_debt(&self, hours: &[HourSummary]) -> i32 {
        hours.last().map(|h| h.debt).unwrap_or(0)
    }

    fn current_date(&self) -> NaiveDate {
        self.config.user_date_time.date()
    }
}