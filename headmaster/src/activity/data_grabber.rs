use serde::{Serialize, Deserialize};
use priestess::{FitbitActivityGrabber, FitbitAuthData, FitbitToken, TokenJson, ActivityGrabber, SleepInterval, HourlyActivityStats, ActivityGrabberError};
use chrono::NaiveDate;
use failure::Error;
use log::warn;
use std::marker::PhantomData;

use crate::proto::Error as ServiceError;
use crate::db::{DbExecutor, UpdateSettingsFitbit, GetSettingsFitbit, GetCachedFitbitResponse, PutCachedFitbitResponse, models::UpdateFitbitCredentials};

use tokio_async_await::compat::backward::Compat;
use actix_web_async_await::await;
use actix_web::actix::{Message, Actor, Context, Handler, Addr, ResponseFuture};

#[derive(Debug, Serialize, Deserialize)]
pub struct Data {
    pub sleep_intervals: Vec<SleepInterval>,
    pub hourly_activity: Vec<HourlyActivityStats>,
}

pub struct DataGrabberExecutor {
    db: Addr<DbExecutor>,
}

impl DataGrabberExecutor {
    pub fn new(db: Addr<DbExecutor>) -> Self {
        Self { db }
    }
}

impl Actor for DataGrabberExecutor {
    type Context = Context<Self>;
}

pub struct GetData<A: ActivityGrabber> {
    user_id: i64,
    date: NaiveDate,
    _marker: std::marker::PhantomData<A>
}

impl<A: ActivityGrabber> Message for GetData<A>
    where A::Token: 'static
{
    type Result = Result<Data, Error>;
}

impl GetData<FitbitActivityGrabber> {
    pub async fn get_data(self, db: Addr<DbExecutor>) -> Result<Data, Error> {
        // Query cache for data
        let cached = await!(db.send(GetCachedFitbitResponse(self.user_id)))??
            .and_then(|s| serde_json::from_str(&s).ok());

        if let Some(cached) = cached {
            return Ok(cached);
        }

        // Load fitbit credentials for the user
        let mut fitbit = await!(db.send(GetSettingsFitbit(self.user_id)))??;

        // Check if there is no token
        let fitbit_token = fitbit.client_token.take()
            .ok_or_else(|| {
                warn!("token not found");
                ServiceError::TokenExpired
            })?;

        // Deserialize token
        let fitbit_token = FitbitToken::from_json(&fitbit_token)
            .map_err(|e| {
                warn!("failed to deserialize token: {}", e);
                ServiceError::TokenExpired
            })?;

        // Construct AuthData for FitbitActivityGrabber
        let auth_data = FitbitAuthData {
            id: fitbit.client_id,
            secret: fitbit.client_secret,
            token: fitbit_token,
        };

        // Authenticate and get auth token
        let grabber = Self::authenticate(auth_data)?;
        let token = Clone::clone(grabber.get_token());
        let token = serde_json::to_string(&token)?;

        // Update auth token
        let req = db.send(UpdateSettingsFitbit::new(
            self.user_id,
            UpdateFitbitCredentials {
                client_token: Some(token),
                ..Default::default()
            }
        ));
        await!(req)??;

        // Fetch data
        let hourly_activity = grabber.fetch_hourly_activity(self.date)?;
        let sleep_intervals = grabber.fetch_sleep_intervals(self.date)?;

        let data = Data {
            sleep_intervals,
            hourly_activity,
        };

        // Update cache (panic here is definitely highly unlikely and should crash the server if happens)
        let new_cache = serde_json::to_string(&data)
            .expect("failed to encode data into JSON");
        await!(db.send(PutCachedFitbitResponse(self.user_id, new_cache)))??;

        Ok(data)
    }
}

impl<A: ActivityGrabber> GetData<A> {
    pub fn new(user_id: i64, date: NaiveDate) -> Self {
        Self {
            user_id,
            date,
            _marker: PhantomData
        }
    }

    fn authenticate(auth_data: A::AuthData) -> Result<A, Error> {
        let grabber = A::new(&auth_data)
            // Convert NewNewToken error into TokenExpired error, so it would be handled correctly by webserver
            .map_err(|e| {
                match e.downcast::<ActivityGrabberError>() {
                    Ok(age) => match age {
                        ActivityGrabberError::NeedNewToken => ServiceError::TokenExpired.into(),
                    },
                    Err(err) => err,
                }
            })?;

        Ok(grabber)
    }
}

impl Handler<GetData<FitbitActivityGrabber>> for DataGrabberExecutor {
    type Result = ResponseFuture<Data, Error>;

    fn handle(&mut self, msg: GetData<FitbitActivityGrabber>, _: &mut Self::Context) -> Self::Result {
        Box::new(Compat::new(msg.get_data(self.db.clone())))
    }
}




