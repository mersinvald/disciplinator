use chrono::{DateTime, Local, NaiveDate, NaiveTime, Timelike};
use failure::Error;
use log::{debug, error, info};
use tiny_http::{Response, Server};

use headmaster::{HourSummary, State, Summary};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenStore,
};

mod config;
use crate::config::Config;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Clone, Debug, StructOpt)]
#[structopt(
    name = "headmaster",
    about = "Disciplinator server-side FitBit API mediator"
)]
struct Options {
    /// Plugins directory path, default = "./plugins"
    #[structopt(
        short = "c",
        long = "config",
        default_value = "./headmaster.toml",
        parse(from_os_str)
    )]
    pub config_path: PathBuf,
}

fn main() -> Result<(), Error> {
    env_logger::init();

    // Load args
    let options = Options::from_args();

    // Load config
    let config = Config::load(options.config_path)?;

    // Connect to Fitbit API
    let auth_data = load_auth_data(&config)?;
    let grabber = FitbitActivityGrabber::new(&auth_data)?;

    // Save refreshed token
    let token = grabber.get_token();
    token.save(".fitbit_token")?;

    // Spin up the http server
    let server = Server::http(&config.network.addr)
        .map_err(|e| panic!("failed to startup the http server: {}", e))
        .unwrap();

    // Create a headmaster instance containing the main debt computation logic
    let mut master = Headmaster::new(grabber, config);

    for request in server.incoming_requests() {
        let summary = master.current_summary();
        match summary {
            Ok(summary) => request.respond(
                Response::from_string(serde_json::to_string(&summary)?).with_status_code(200),
            )?,
            Err(err) => request.respond(
                Response::from_string(format!("failed to get status: {}", err))
                    .with_status_code(503),
            )?,
        }
    }

    Ok(())
}

struct Headmaster<A> {
    config: Config,
    grabber: A,
    cache: SummaryCache,
}

#[derive(Debug, Default, Copy, Clone)]
struct Hour {
    hour: u32,
    complete: bool,
    active_minutes: u32,
    accounted_active_minutes: u32,
    tracking_disabled: bool,
    debt: u32,
}

impl From<Hour> for HourSummary {
    fn from(hour: Hour) -> HourSummary {
        HourSummary {
            hour: hour.hour,
            debt: hour.debt,
            active_minutes: hour.active_minutes,
            tracking_disabled: hour.tracking_disabled,
            complete: hour.complete,
        }
    }
}

struct SummaryCache {
    summary: Option<Summary>,
    time: DateTime<Local>,
}

impl SummaryCache {
    pub fn empty() -> Self {
        SummaryCache {
            summary: None,
            time: Local::now() - chrono::Duration::hours(1),
        }
    }

    pub fn set(&mut self, summary: Summary) {
        self.summary = Some(summary);
        self.time = Local::now();
    }

    pub fn get(&self) -> Option<Summary> {
        if Local::now().signed_duration_since(self.time) < chrono::Duration::minutes(1) {
            self.summary.clone()
        } else {
            None
        }
    }
}

impl<A: ActivityGrabber> Headmaster<A> {
    pub fn new(grabber: A, config: Config) -> Self {
        Headmaster {
            config,
            grabber,
            cache: SummaryCache::empty(),
        }
    }

    pub fn current_hour_and_day_log(&self) -> Result<(HourSummary, Vec<HourSummary>), Error> {
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
            Hour {
                complete: true,
                tracking_disabled: true,
                ..Default::default()
            }
        });

        let current_hour_summary = HourSummary {
            hour: last_hour.hour,
            debt,
            complete: last_hour.complete,
            tracking_disabled: last_hour.tracking_disabled,
            active_minutes: last_hour.active_minutes,
        };

        let day_log = hours.into_iter().map(HourSummary::from).collect();

        Ok((current_hour_summary, day_log))
    }

    pub fn current_summary(&mut self) -> Result<Summary, Error> {
        // Query cache
        if let Some(summary) = self.cache.get() {
            info!("less then a minute passed since last request, using the cached summary");
            return Ok(summary);
        }

        // Get last stats from Fitbit
        let (hour, day_log) = self.current_hour_and_day_log()?;

        // Calculate the correct system state:
        // 1. debt > 0 and user haven't been active >= max hourly accounted time => DebtCollection
        // 2. debt > 0 and user can't log more time this hour due to the limit => DebtCollectionPaused
        // 3. no debt => Normal
        let max_accounted = self.config.limits.max_accounted_active_time;
        let state = if hour.debt > 0 && hour.active_minutes < max_accounted {
            State::DebtCollection(hour)
        } else if hour.debt > 0 && hour.active_minutes >= max_accounted {
            State::DebtCollectionPaused(hour)
        } else {
            State::Normal(hour)
        };

        let summary = Summary { state, day_log };

        // Put the summary into the cache
        self.cache.set(summary.clone());

        Ok(summary)
    }

    fn get_active_minutes_hourly(&self) -> Result<Vec<Hour>, Error> {
        let data = self
            .grabber
            .fetch_hourly_activity(Self::current_date())?
            .iter()
            .map(|h| Hour {
                hour: h.hour,
                complete: h.complete,
                active_minutes: h.active_minutes,
                accounted_active_minutes: h.active_minutes,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        Ok(data)
    }

    fn exclude_inactive_hours(&self, mut hours: Vec<Hour>) -> Result<Vec<Hour>, Error> {
        // Fetch the sleeping intervals from FitBit API
        let mut sleep_intervals = self.grabber.fetch_sleep_intervals(Self::current_date())?;

        debug!("sleep intervals: {:#?}", sleep_intervals);

        // If no data there, fallback to config defined day start time
        if sleep_intervals.is_empty() {
            sleep_intervals.push(SleepInterval {
                start: NaiveTime::from_hms(0, 0, 0),
                end: self.config.day.day_begins_at,
            })
        }

        // Calculate day end
        let day_end = sleep_intervals.iter().fold(None, |day_end, interval| {
            if day_end.is_none() {
                Some(interval.end + chrono::Duration::hours(self.config.day.day_length))
            } else {
                day_end.map(|time| time + (interval.end - interval.start))
            }
        });

        debug!("day ends at: {:?}", day_end);

        // Add the day end interval as well,
        sleep_intervals.push(SleepInterval {
            start: day_end.unwrap_or(self.config.day.day_ends_at),
            end: NaiveTime::from_hms(23, 59, 59),
        });

        hours.iter_mut().for_each(|h| {
            for interval in &sleep_intervals {
                // Zero debt, zero overtime
                let activity_during_sleep = self.config.limits.minimum_active_time;
                if h.hour >= interval.start.hour() && h.hour < interval.end.hour() {
                    h.accounted_active_minutes = activity_during_sleep;
                    h.tracking_disabled = true;
                } else if h.hour == interval.end.hour() {
                    h.accounted_active_minutes =
                        u32::min(interval.end.minute(), activity_during_sleep);
                    if h.accounted_active_minutes == activity_during_sleep {
                        h.tracking_disabled = true;
                    }
                }
            }
        });

        Ok(hours)
    }

    fn normalize_by_threshold(&self, mut hours: Vec<Hour>) -> Vec<Hour> {
        hours.iter_mut().for_each(|h| {
            let limits = &self.config.limits;
            h.accounted_active_minutes =
                u32::min(h.accounted_active_minutes, limits.max_accounted_active_time);
            h.debt = u32::min(h.debt, limits.debt_limit);
        });

        hours
    }

    fn calculate_debt_hourly(&self, mut hours: Vec<Hour>) -> Vec<Hour> {
        let limits = &self.config.limits;

        // Calculate first hour activity debt
        hours[0].debt = limits
            .minimum_active_time
            .checked_sub(hours[0].accounted_active_minutes)
            .unwrap_or(0);

        for i in 1..hours.len() {
            // Next hour debt is previous hour debt + current hour default debt
            let current_hour_minimum = if hours[i].complete {
                limits.minimum_active_time
            } else {
                0
            };

            hours[i].debt = (current_hour_minimum + hours[i - 1].debt)
                .checked_sub(hours[i].accounted_active_minutes)
                .unwrap_or(0)
        }

        hours
    }

    fn calculate_debt(&self, hours: &[Hour]) -> u32 {
        hours.last().map(|h| h.debt).unwrap_or(0)
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
