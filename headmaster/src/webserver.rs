use futures::Future;
use uuid::Uuid;
use std::cell::RefCell;
use log::debug;
use std::rc::Rc;
use std::collections::HashMap;
use serde::Serialize;

use actix_web_async_await::{await, compat, compat2};
use actix_web::actix;
use actix_web::actix::{Addr, Message};
use actix_web::{
    server,
    http::Method,
    App,
    Error,
    HttpRequest,
    HttpResponse,
    ResponseError,
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
};

use chrono::NaiveDateTime;

use crate::proto::Summary;
use priestess::FitbitActivityGrabber;

use crate::config::Config;
use crate::db::{self, DbExecutor};
use crate::proto::http;
use crate::proto::Error as ServiceError;
use crate::proto::Response;
use crate::activity::eval::DebtEvaluatorExecutor;
use crate::activity::eval;

type HttpResult = Result<HttpResponse, ServiceError>;

fn create_response<D, E>(result: Result<D, E>) -> HttpResult
    where D: Serialize,
          ServiceError: From<E>
{
    match result {
        Ok(data) => Ok(HttpResponse::Ok().json(Response::data(data))),
        Err(err) => Err(ServiceError::from(err))
    }
}

async fn db_response<D, E, M>(state: &AppState, message: M) -> HttpResult
    where M: Message<Result = Result<D, E>> + Send + 'static,
          D: Serialize + Send + 'static,
          E: Send + 'static,
          ServiceError: From<E>,
          DbExecutor: actix::Handler<M>,
{
    let db_result = await!(
        state.db.send(message)
    )?;

    create_response(db_result)
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

    create_response(db_result.map(map))
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
    let summary = await!(do_get_summary(req.state(), timestamp, user_id))?;
    Ok(HttpResponse::Ok().json(Response::data(summary)))
}

async fn get_state(path: Path<i64>, req: HttpRequest<AppState>) -> HttpResult {
    let user_id = req.user_id()?;
    let timestamp = path.into_inner();
    let summary = await!(do_get_summary(req.state(), timestamp, user_id))?;
    Ok(HttpResponse::Ok().json(Response::data(summary.status)))
}

async fn do_get_summary(state: &AppState, timestamp: i64, user_id: i64) -> Result<Summary, ServiceError> {
    let datetime = NaiveDateTime::from_timestamp(timestamp, 0);
    debug!("client time: {}", datetime);

    // Request summary and new token from Headmaster actor
    let message = eval::GetSummary::<FitbitActivityGrabber>::new(user_id, datetime);
    let summary = await!(state.evaluator.send(message))??;

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

async fn validate_email(_req: HttpRequest<AppState>) -> HttpResult {
    Ok(ServiceError::NotImplemented.error_response())
}

pub fn start(config: Config, db_addr: Addr<DbExecutor>, evaluator: Addr<DebtEvaluatorExecutor>) -> Result<Addr<Server>, Error> {
    let server = server::new(move || {

        let json_config = move |cfg: &mut (JsonConfig<AppState>, ())| {
            cfg.0.limit(4096)
                .error_handler(|err, _req| {
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
                evaluator: evaluator.clone(),
                token_map: Rc::new(RefCell::new(HashMap::new())),
            })
            .middleware(middleware::Logger::default())
            .prefix("/1")
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
    }).bind(&config.listen_on)?
        .start();

    Ok(server)
}

struct AppState {
    db: Addr<DbExecutor>,
    evaluator: Addr<DebtEvaluatorExecutor>,
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
        .send(db::GetUserByToken(token))
        .from_err()
        .and_then(move |res| match res {
            Ok(user) => {
                let mut token_map = token_map.borrow_mut();
                token_map.insert(token, user.id);
                Ok(None)
            },
            Err(_) => Ok(Some(ServiceError::Unauthorized.error_response()))
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
                error: "no token -> id pair in state hashmap after AuthMiddleware".into()
            })?;
        Ok(*id)
    }
}
