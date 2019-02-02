use priestess::{ActivityGrabber, SleepInterval};
use chrono::NaiveDateTime;
use chrono::{NaiveTime, Timelike};
use failure::Error;
use log::{info, debug, error};
use std::marker::PhantomData;

use crate::proto::activity::{Summary, HourSummary, Status};
use crate::db::{DbExecutor, GetSettings};
use crate::activity::data_grabber::{DataGrabberExecutor, GetData, Data as ActivityData};

use tokio_async_await::compat::backward::Compat;
use actix_web_async_await::await;
use actix_web::actix::{Message, Actor, Context, Handler, Addr, ResponseFuture};

#[derive(Clone)]
pub struct DebtEvaluatorExecutor {
    db: Addr<DbExecutor>,
    grabber: Addr<DataGrabberExecutor>,
}

impl DebtEvaluatorExecutor {
    pub fn new(db: Addr<DbExecutor>, grabber: Addr<DataGrabberExecutor>) -> Self {
        Self { db, grabber }
    }
}

impl Actor for DebtEvaluatorExecutor {
    type Context = Context<Self>;
}

pub struct GetSummary<G: ActivityGrabber> {
    user_id: i64,
    datetime: NaiveDateTime,
    _marker: PhantomData<G>,
}

impl<G: ActivityGrabber> GetSummary<G> {
    pub fn new(user_id: i64, datetime: NaiveDateTime) -> Self {
        GetSummary {
            user_id,
            datetime,
            _marker: PhantomData
        }
    }
}

impl<G: ActivityGrabber> Message for GetSummary<G>
    where G::Token: 'static
{
    type Result = Result<Summary, Error>;
}

impl<A: ActivityGrabber> Handler<GetSummary<A>> for DebtEvaluatorExecutor
    where A: 'static
{
    type Result = ResponseFuture<Summary, Error>;

    fn handle(&mut self, msg: GetSummary<A>, _: &mut Self::Context) -> Self::Result {
        Box::new(Compat::new(self.clone().evaluate(msg)))
    }
}

impl DebtEvaluatorExecutor {
    async fn evaluate<A: ActivityGrabber + 'static>(self, msg: GetSummary<A>) -> Result<Summary, Error> {
        let settings = await!(self.db.send(GetSettings(msg.user_id)))??;

        let config = DebtEvaluatorConfig {
            minimum_active_time: settings.hourly_activity_goal as u32,
            max_accounted_active_minutes: settings.hourly_activity_limit
                .unwrap_or(settings.hourly_activity_goal * 3) as u32,
            debt_limit: settings.hourly_debt_limit
                .unwrap_or(settings.hourly_activity_goal * 3) as u32,
            day_begins_at: settings.day_starts_at,
            day_ends_at: settings.day_ends_at,
            day_length: settings.day_length
                .map(|l| l as u32)
                .unwrap_or(settings.day_ends_at.hour() - settings.day_starts_at.hour()),
        };

        let data = await!(self.grabber.send(GetData::new(
            msg.user_id,
            msg.datetime.date(),
        )))??;

        let evaluator = DebtEvaluator::new(
            config,
            data
        );

        let summary = evaluator.current_summary();

        Ok(summary)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct DebtEvaluatorConfig {
    pub minimum_active_time: u32,
    pub max_accounted_active_minutes: u32,
    pub debt_limit: u32,
    pub day_begins_at: NaiveTime,
    pub day_ends_at: NaiveTime,
    pub day_length: u32,
}

pub struct DebtEvaluator {
    config: DebtEvaluatorConfig,
    data: ActivityData,
}

impl DebtEvaluator {
    pub fn new(config: DebtEvaluatorConfig, data: ActivityData) -> Self {
        DebtEvaluator {
            config,
            data,
        }
    }
}

impl DebtEvaluator {
    pub fn current_hour_and_day_log(&self) -> (HourSummary, Vec<HourSummary>) {
        let hours = self.get_active_minutes_hourly();
        debug!("ABSOLUTE DEBT: \n{:#?}", hours);
        let hours = self.exclude_inactive_hours(hours);
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

        (last_hour, hours)
    }

    pub fn current_summary(&self) -> Summary {
        // Get last stats from Fitbit
        let (hour, day_log) = self.current_hour_and_day_log();

        // Calculate the correct system state:
        // 1. debt > 0 and user haven't been active >= max hourly accounted time => DebtCollection
        // 2. debt > 0 and user can't log more time this hour due to the limit => DebtCollectionPaused
        // 3. no debt => Normal
        let max_accounted = self.config.max_accounted_active_minutes;
        let state = if hour.debt > 0 && hour.active_minutes < max_accounted {
            Status::DebtCollection(hour)
        } else if hour.debt > 0 && hour.active_minutes >= max_accounted {
            Status::DebtCollectionPaused(hour)
        } else {
            Status::Normal(hour)
        };

        Summary { status: state, day_log }
    }

    fn get_active_minutes_hourly(&self) -> Vec<HourSummary> {
        self.data
            .hourly_activity
            .iter()
            .map(|h| HourSummary {
                hour: h.hour,
                complete: h.complete,
                active_minutes: h.active_minutes,
                accounted_active_minutes: h.active_minutes,
                ..Default::default()
            })
            .collect::<Vec<_>>()
    }

    fn exclude_inactive_hours(&self, mut hours: Vec<HourSummary>) -> Vec<HourSummary> {
        let mut sleep_intervals = self.data.sleep_intervals.clone();
        debug!("sleep intervals: {:#?}", self.data.sleep_intervals);

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
                Some(interval.end + chrono::Duration::hours(i64::from(self.config.day_length)))
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
                if (h.hour >= interval.start.hour() && h.hour < interval.end.hour())
                || (h.hour == interval.end.hour() && interval.end.minute() > (60 - self.config.minimum_active_time))
                {
                    h.tracking_disabled = true;
                }
            }
        });

        for over in &self.data.activity_overrides {
            hours[over.hour as usize].tracking_disabled = !over.is_active;
        }

        let activity_during_sleep = self.config.minimum_active_time;
        hours.iter_mut().for_each(|h| {
            if h.tracking_disabled {
                h.accounted_active_minutes = u32::max(h.active_minutes, activity_during_sleep)
            }
        });

        hours
    }

    fn normalize_by_threshold(&self, mut hours: Vec<HourSummary>) -> Vec<HourSummary> {
        hours.iter_mut().for_each(|h| {
            h.accounted_active_minutes =
                u32::min(h.accounted_active_minutes, self.config.max_accounted_active_minutes);
            h.debt = u32::min(h.debt, self.config.debt_limit);
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

    fn calculate_debt(&self, hours: &[HourSummary]) -> u32 {
        hours.last().map(|h| h.debt).unwrap_or(0)
    }
}