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
use serde::{Serialize, Deserialize};

use actix_web_async_await::{await, compat, compat2};
use actix_web::actix;
use actix_web::actix::{SyncArbiter, Addr, Message};
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
    Json,
    Path,
    dev::JsonConfig,
    State as RequestState,
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

type HttpResult = Result<HttpResponse, Error>;

fn create_response<D, E>(result: Result<D, E>) -> HttpResponse
    where D: Serialize,
          ServiceError: From<E>
{
    match result {
        Ok(data) => HttpResponse::Ok().json(Response::data(data)),
        Err(err) => ServiceError::from(err).error_response()
    }
}

async fn db_response<D, E, M>(state: &AppState, message: M) -> HttpResult
    where M: Message<Result = Result<D, E>> + Send + 'static,
          <M as Message>::Result: Send,
          D: Serialize + 'static,
          E: 'static,
          ServiceError: From<E>,
          DbExecutor: actix::Handler<M>,
{
    let db_result = await!(
        state.db.send(message)
    )?;

    Ok(create_response(db_result))
}

async fn db_response_map<D, E, M, F>(state: &AppState, message: M, map: F) -> HttpResult
    where M: Message<Result = Result<D, E>> + Send + 'static,
          <M as Message>::Result: Send,
          D: Serialize + 'static,
          E: 'static,
          ServiceError: From<E>,
          DbExecutor: actix::Handler<M>,
          F: Fn(D) -> D + 'static,
{
    let db_result = await!(
        state.db.send(message)
    )?;

    Ok(create_response(db_result.map(map)))
}

async fn register(json: Json<http::Register>, state: RequestState<AppState>) -> HttpResult {
    await!(db_response(&state, db::CreateUser::from_body(json)))
}

async fn login(json: Json<http::Login>, state: RequestState<AppState>) -> HttpResult  {
    await!(db_response(&state, db::LoginUser::from_body(json)))
}

async fn get_summary(path: Path<i64>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    let timestamp = path.into_inner();
    let summary = await!(do_get_summary(req.state(), user_id, timestamp))?;
    Ok(HttpResponse::Ok().json(Response::data(summary)))
}

async fn get_state(path: Path<i64>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    let timestamp = path.into_inner();
    let summary = await!(do_get_summary(req.state(), user_id, timestamp))?;
    Ok(HttpResponse::Ok().json(Response::data(summary.state)))
}

async fn do_get_summary(state: &AppState, timestamp: i64, user_id: i64) -> Result<Summary, Error> {
    let datetime = NaiveDateTime::from_timestamp(timestamp, 0);
    debug!("client time: {}", datetime);

    // Get needed actors from state
    let db = &state.db;
    let headmaster = &state.headmaster;

    // Fetch settings and Fitbit credentials
    let settings = await!(db.send(db::GetSettings(user_id)))??;
    let mut fitbit = await!(db.send(db::GetSettingsFitbit(user_id)))??;

    // Check if there is no token
    let fitbit_token = fitbit.client_token.take()
        .ok_or(ServiceError::TokenExpired)?;

    // Deserialize token
    let fitbit_token = FitbitToken::from_json(&fitbit_token)
        .map_err(|_| ServiceError::TokenExpired)?;

    // Construct headmaster config
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

    // Construct fitbit grabber authentication data
    let auth_data = FitbitAuthData {
        id: fitbit.client_id,
        secret: fitbit.client_secret,
        token: fitbit_token,
    };

    // Request summary and new token from Headmaster actor
    let message = master::GetSummary::<FitbitActivityGrabber>::new(headmaster_config, auth_data);
    let request = headmaster.send(message);
    let (summary, new_token) = await!(request)??;

    // Update Fitbit token in database
    let new_token = new_token.to_json();
    let changeset = db::models::UpdateFitbitCredentials {
        client_token: Some(Some(new_token)),
        ..Default::default()
    };
    let message = db::UpdateSettingsFitbit::new(user_id, changeset);
    await!(db.send(message))?;

    // Return summary
    Ok(summary)
}

async fn get_settings(req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    await!(db_response(req.state(), db::GetSettings(user_id)))
}

async fn update_settings(json: Json<db::models::UpdateSettings>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    await!(db_response(req.state(), db::UpdateSettings::new(user_id, json)))
}

async fn get_settings_fitbit(req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    await!(db_response(req.state(), db::GetSettingsFitbit(user_id)))
}

async fn update_settings_fitbit(json: Json<db::models::UpdateFitbitCredentials>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    await!(db_response(req.state(), db::UpdateSettingsFitbit::from_json(user_id, json)))
}

async fn get_user(req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    let response = db_response_map(req.state(), db::GetUser(user_id), |mut user| {
        // Clean the passwd hash
        user.passwd_hash.clear();
        user
    });
    await!(response)
}

async fn update_user(json: Json<http::UpdateUser>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    let response = db_response_map(req.state(), db::UpdateUser::from_json(user_id, json), |mut user| {
        // Clean the passwd hash
        user.passwd_hash.clear();
        user
    });
    await!(response)
}

async fn validate_email(req: HttpRequest<AppState>) -> HttpResult {
    Ok(ServiceError::NotImplemented.error_response())
}

pub fn start(config: Config, db_addr: Addr<DbExecutor>, headmaster: Addr<HeadmasterExecutor>) -> Result<Addr<Server>, Error> {
    let server = server::new(move || {

        let json_config = move |cfg: &mut (JsonConfig<AppState>, ())| {
            cfg.0.limit(4096)
                .error_handler(|err, req| {
                    let err_msg = format!("{}", err);
                    actix_web::error::InternalError::from_response(
                        err, ServiceError::InvalidPayload {
                            error: err_msg,
                        }.error_response()
                    ).into()
                });
        };

        App::with_state(AppState {
                db: db_addr.clone(),
                headmaster: headmaster.clone(),
                token_map: Rc::new(RefCell::new(HashMap::new())),
            })
            .middleware(middleware::Logger::default())
            .resource("/register", move |r| r.method(Method::POST).with_config(compat2(register), json_config))
            .resource("/login", move |r| r.method(Method::POST).with_config(compat2(login), json_config))
            .resource("/summary/{timestamp}", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).with(compat2(get_summary));
            })
            .resource("/state/{timestamp}", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).with(compat2(get_state));
            })
            .resource("/settings", move |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).with(compat(get_settings));
                r.method(Method::POST).with_config(compat2(update_settings), json_config);
            })
            .resource("/settings/fitbit", move |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::POST).with_config(compat2(update_settings_fitbit), json_config);
                r.method(Method::GET).with(compat(get_settings_fitbit));
            })
            .resource("/user", move |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).with(compat(get_user));
                r.method(Method::POST).with_config(compat2(update_user), json_config);
            })
            .resource("/user/validate_email/{email_token}", |r| r.method(Method::GET).with(compat(validate_email)))
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
