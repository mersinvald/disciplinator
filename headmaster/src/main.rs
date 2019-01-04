use chrono::{DateTime, Local, NaiveDate, NaiveTime, Timelike};
use failure::Error;
use log::{debug, error, info};
use tiny_http::{Response, Server};

use headmaster::{CurrentHourSummary, State};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenStore,
};

mod config;
use crate::config::Config;

fn main() -> Result<(), Error> {
    env_logger::init();

    // Load config
    let config = Config::load("headmaster.toml")?;

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
        let state = master.current_state();
        match state {
            Ok(state) => request.respond(
                Response::from_string(serde_json::to_string(&state)?).with_status_code(200),
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
    cache: StateCache,
}

#[derive(Debug, Default, Copy, Clone)]
struct Hour {
    hour: u32,
    complete: bool,
    active_minutes: u32,
    accounted_active_minutes: u32,
    debt: u32,
}

struct StateCache {
    state: Option<State>,
    time: DateTime<Local>,
}

impl StateCache {
    pub fn empty() -> Self {
        StateCache {
            state: None,
            time: Local::now() - chrono::Duration::hours(1),
        }
    }

    pub fn set(&mut self, state: State) {
        self.state = Some(state);
        self.time = Local::now();
    }

    pub fn get(&self) -> Option<State> {
        if Local::now().signed_duration_since(self.time) < chrono::Duration::minutes(1) {
            self.state
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
            cache: StateCache::empty(),
        }
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
                    self.config.limits.max_accounted_active_time
                }),
        })
    }

    pub fn current_state(&mut self) -> Result<State, Error> {
        // Query cache
        if let Some(state) = self.cache.get() {
            info!("less then a minute passed since last request, using the cached state");
            return Ok(state);
        }

        // Get last stats from Fitbit
        let stat = self.current_hour_summary()?;

        // Calculate the correct system state:
        // 1. debt > 0 and user haven't been active >= max hourly accounted time => DebtCollection
        // 2. debt > 0 and user can't log more time this hour due to the limit => DebtCollectionPaused
        // 3. no debt => Normal
        let max_accounted = self.config.limits.max_accounted_active_time;
        let state = if stat.debt > 0 && stat.active_minutes < max_accounted {
            State::DebtCollection(stat)
        } else if stat.debt > 0 && stat.active_minutes >= max_accounted {
            State::DebtCollectionPaused(stat)
        } else {
            State::Normal(stat)
        };

        // Put the state into the cache
        self.cache.set(state);

        Ok(state)
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
                } else if h.hour == interval.end.hour() {
                    h.accounted_active_minutes = u32::min(interval.end.minute(), activity_during_sleep);
                }
            }
        });

        Ok(hours)
    }

    fn normalize_by_threshold(&self, mut hours: Vec<Hour>) -> Vec<Hour> {
        hours.iter_mut().for_each(|h| {
            let limits = &self.config.limits;
            h.accounted_active_minutes = u32::min(h.accounted_active_minutes, limits.max_accounted_active_time);
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
        let limits = &self.config.limits;
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
