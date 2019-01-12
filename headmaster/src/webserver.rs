use failure::{Fail};
use futures::Future;
use futures::future;
use uuid::Uuid;
use std::cell::RefCell;
use std::str::FromStr;
use log::{info, debug};
use std::sync::Arc;
use std::rc::Rc;
use std::collections::HashMap;

use actix_web::actix::{SyncArbiter, Addr};
use actix_web::{
    server,
    http::{Method, header},
    App,
    Error,
    HttpRequest,
    HttpResponse,
    ResponseError,
    Responder,
    HttpMessage,
    AsyncResponder,
};
use actix_net::server::Server;

use actix_web::middleware::{
    self,
    Middleware,
    Started,
    Response as MwResponse,
    session::{
        SessionBackend,
        SessionImpl,
        SessionStorage,
        Session,
    }
};

use chrono::{NaiveDateTime, Timelike};

use crate::proto::{HourSummary, State, Summary};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenJson,
};

use crate::config::Config;
use crate::db::{self, DbExecutor};
use crate::proto::http;
use crate::proto::Error as ServiceError;
use crate::proto::Response;
use crate::master::HeadmasterExecutor;
use crate::master;

use crate::db::models::User;

macro_rules! try_or_respond {
    ($req:expr) => {
        match $req {
            Ok(id) => id,
            Err(err) => return Box::new(future::ok(ServiceError::from(err).error_response()))
        }
    }
}

fn register(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: http::Register| {
            db.send(db::CreateUser::from(body))
                .from_err()
                .and_then(|res| match res {
                    Ok(user_id) => Ok(HttpResponse::Ok().json(Response::data(user_id))),
                    Err(e) => Ok(ServiceError::from(e).error_response())
                })
        })
        .responder()
}

fn login(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: http::Login| {
            db.send(db::LoginUser::from(body))
                .from_err()
                .and_then(|res| match res {
                    Ok(user_id) => Ok(HttpResponse::Ok().json(Response::data(user_id))),
                    Err(e) => Ok(ServiceError::from(e).error_response())
                })
        })
        .responder()
}

fn get_summary(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    do_get_summary(req, user_id)
        .then(|res| match res {
            Ok(summary) => Ok(HttpResponse::Ok().json(Response::data(summary))),
            Err(e) => Ok(ServiceError::from(e).error_response())
        })
        .responder()
}

fn get_state(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    do_get_summary(req, user_id)
        .then(|res| match res {
            Ok(summary) => Ok(HttpResponse::Ok().json(Response::data(summary.state))),
            Err(e) => Ok(ServiceError::from(e).error_response())
        })
        .responder()
}

type SummaryFuture = Box<dyn Future<Item = Summary, Error = failure::Error>>;

fn do_get_summary(req: &HttpRequest<AppState>, user_id: i64) -> SummaryFuture {
    let datetime = req.match_info()
        .get("timestamp")
        .and_then(|s| i64::from_str(s).ok())
        .map(|ts| NaiveDateTime::from_timestamp(ts, 0));

    let datetime = match datetime {
        Some(dt) => dt,
        None => return Box::new(future::result(Err(ServiceError::InvalidSetting {
            key: "timestamp".into(),
            hint: "local time in seconds since Unix Epoch".into()
        }.into())))
    };

    debug!("client time: {}", datetime);

    let db = req.state().db.clone();

    let settings = req.state().db
        .send(db::GetSettings(user_id))
        .from_err()
        .and_then(|res| match res {
            Ok(settings) => Ok(settings),
            Err(err) => Err(err)
        });

    let fitbit = req.state().db
        .send(db::GetSettingsFitbit(user_id))
        .map_err(failure::Error::from)
        // Check if there is token and flatten error
        .and_then(|res| match res {
            Ok(fitbit) => {
                if fitbit.client_token.is_none() {
                    debug!("no token in database for user {}", fitbit.user_id);
                    Err(ServiceError::TokenExpired.into())
                } else {
                    Ok(fitbit)
                }
            },
            Err(err) => Err(err)
        });

    let headmaster = req.state().headmaster.clone();

    let summary_and_token = settings.join(fitbit)
        .and_then(move |(settings, fitbit)| -> Box<dyn Future<Item = (Summary, FitbitToken), Error = failure::Error>> {
            // Deserialize token
            let token = fitbit.client_token.expect("ISE: token option is not cleared");
            let fitbit_token = match FitbitToken::from_json(&token) {
                Ok(token) => token,
                Err(err) => return Box::new(future::err(ServiceError::TokenExpired.into()))
            };

            let headmaster_config = master::HeadmasterConfig {
                // Guaranteed to be < 180 by checks in database
                minimum_active_time: settings.hourly_activity_goal as u32,
                max_accounted_active_minutes: settings.hourly_activity_limit.unwrap_or(settings.hourly_activity_goal * 3) as u32,
                debt_limit: settings.hourly_debt_limit.unwrap_or(settings.hourly_activity_goal * 3) as u32,
                day_begins_at: settings.day_starts_at ,
                day_ends_at: settings.day_ends_at,
                day_length: settings.day_length.map(|l| l as u32).unwrap_or(settings.day_ends_at.hour() - settings.day_starts_at.hour()),
                user_date_time: datetime,
            };

            let auth_data = FitbitAuthData {
                id: fitbit.client_id,
                secret: fitbit.client_secret,
                token: fitbit_token,
            };

            let future = headmaster.send(master::GetSummary::<FitbitActivityGrabber>::new(headmaster_config, auth_data))
                .map_err(failure::Error::from)
                // flatten error
                .and_then(|res| res);

            Box::new(future)
        });

    let summary = summary_and_token
        .and_then(move |(summary, fitbit_token)| {
            db.send(db::UpdateSettingsFitbit::new(
                user_id, db::models::UpdateFitbitCredentials {
                    client_token: Some(Some(fitbit_token.to_json())),
                    ..Default::default()
                }))
                .map_err(failure::Error::from)
                .and_then(|_| Ok(summary))
        });

    Box::new(summary)
}

fn get_settings(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    req.state().db
        .send(db::GetSettings(user_id))
        .from_err()
        .and_then(|res| match res {
            Ok(mut settings) => {
                Ok(HttpResponse::Ok().json(Response::data(settings)))
            },
            Err(e) => Ok(ServiceError::from(e).error_response())
        })
        .responder()
}

fn update_settings(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: db::models::UpdateSettings| {
            db.send(db::UpdateSettings::new(user_id, body))
                .from_err()
                .and_then(|res| match res {
                    Ok(settings) => {
                        Ok(HttpResponse::Ok().json(Response::data(settings)))
                    },
                    Err(e) => Ok(ServiceError::from(e).error_response())
                })
        })
        .responder()
}

fn get_settings_fitbit(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    req.state().db
        .send(db::GetSettingsFitbit(user_id))
        .from_err()
        .and_then(|res| match res {
            Ok(mut settings) => {
                Ok(HttpResponse::Ok().json(Response::data(settings)))
            },
            Err(e) => Ok(ServiceError::from(e).error_response())
        })
        .responder()
}

fn update_settings_fitbit(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: db::models::UpdateFitbitCredentials| {
            db.send(db::UpdateSettingsFitbit::new(user_id, body))
                .from_err()
                .and_then(|res| match res {
                    Ok(settings) => {
                        Ok(HttpResponse::Ok().json(Response::data(settings)))
                    },
                    Err(e) => Ok(ServiceError::from(e).error_response())
                })
        })
        .responder()
}

fn get_user(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    req.state().db
        .send(db::GetUser(user_id))
        .from_err()
        .and_then(|res| match res {
            Ok(mut user) => {
                // Clean the passwd hash
                user.passwd_hash.clear();
                Ok(HttpResponse::Ok().json(Response::data(user)))
            },
            Err(e) => Ok(ServiceError::from(e).error_response())
        })
        .responder()
}

fn update_user(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let user_id = try_or_respond!(req.user_id());
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: http::UpdateUser| {
            db.send(db::UpdateUser::new(user_id, body))
                .from_err()
                .and_then(|res| match res {
                    Ok(mut user) => {
                        // Clean the passwd hash
                        user.passwd_hash.clear();
                        Ok(HttpResponse::Ok().json(Response::data(user)))
                    },
                    Err(e) => Ok(ServiceError::from(e).error_response())
                })
        })
        .responder()
}

fn validate_email(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

pub fn start(config: Config, db_addr: Addr<DbExecutor>, headmaster: Addr<HeadmasterExecutor>) -> Result<Addr<Server>, Error> {
    let server = server::new(move || {
        App::with_state(AppState {
                db: db_addr.clone(),
                headmaster: headmaster.clone(),
                token_map: Rc::new(RefCell::new(HashMap::new())),
            })
            .middleware(middleware::Logger::default())
            .prefix("/1")
            .resource("/register", |r| r.method(Method::POST).a(register))
            .resource("/login", |r| r.method(Method::POST).a(login))
            .resource("/summary/{timestamp}", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_summary);
            })
            .resource("/state/{timestamp}", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_state);
            })
            .resource("/settings", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_settings);
                r.method(Method::POST).a(update_settings);
            })
            .resource("/settings/fitbit", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::POST).a(update_settings_fitbit);
                r.method(Method::GET).a(get_settings_fitbit);
            })
            .resource("/user", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_user);
                r.method(Method::POST).a(update_user);
            })
            .resource("/user/validate_email/{email_token}", |r| r.method(Method::GET).a(validate_email))
    }).bind(&config.network.addr)?
        .start();

    Ok(server)
}

struct AppState {
    db: Addr<DbExecutor>,
    headmaster: Addr<HeadmasterExecutor>,
    token_map: Rc<RefCell<HashMap<Uuid, i64>>>,
}

#[derive(Copy, Clone, Debug)]
struct AuthMiddleware;

impl Middleware<AppState> for AuthMiddleware {
    fn start(&self, req: &HttpRequest<AppState>) -> actix_web::Result<Started> {
        // Don't touch options requests
        if req.method() == "OPTIONS" {
            return Ok(Started::Done);
        }

        let token = req.token()?;
        Ok(verify_token(token, req.state())?)
    }
}

fn verify_token(token: Uuid, state: &AppState) -> actix_web::Result<Started> {
    let token_map = state.token_map.clone();
    let user = state.db
        .send(db::GetUserByToken(token.clone()))
        .from_err()
        .and_then(move |res| match res {
            Ok(user) => {
                let mut token_map = token_map.borrow_mut();
                token_map.insert(token, user.id);
                Ok(None)
            },
            Err(e) => Ok(Some(ServiceError::Unauthorized.error_response()))
        });

    Ok(Started::Future(Box::new(user)))
}

trait ExtractTokenHeader {
    fn token(&self) -> Result<Uuid, ServiceError>;
}

impl<T> ExtractTokenHeader for HttpRequest<T> {
    fn token(&self) -> Result<Uuid, ServiceError> {
        use std::str::FromStr;

        let token = self.headers()
            .get("AUTHORIZATION")
            .and_then(|value| value.to_str().ok())
            .ok_or(ServiceError::Unauthorized)?;

        let token = Uuid::from_str(token)
            .map_err(|_| ServiceError::Unauthorized)?;

        Ok(token)
    }
}

trait ExtractUserId {
    fn user_id(&self) -> Result<i64, ServiceError>;
}

impl ExtractUserId for HttpRequest<AppState> {
    fn user_id(&self) -> Result<i64, ServiceError> {
        let token = self.token()?;
        let token_map = self.state()
            .token_map
            .borrow();
        let id = token_map
            .get(&token)
            .ok_or_else(|| ServiceError::Internal {
                error: "no token -> id pair in state hashmap after AuthMiddleware".into(),
                backtrace: String::new(),
            })?;
        Ok(*id)
    }
}
