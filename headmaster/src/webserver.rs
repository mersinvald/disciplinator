use failure::{Fail};
use futures::Future;
use futures::future;
use uuid::Uuid;

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
    session::{
        SessionBackend,
        SessionStorage,
        Session,
    }
};

use headmaster::proto::{HourSummary, State, Summary};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenJson,
};

use crate::config::Config;
use crate::db::{self, DbExecutor};
use crate::proto::http;
use crate::proto::Error as ServiceError;
use crate::proto::Response;

use crate::db::models::User;

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
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn get_state(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn get_settings(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn update_settings(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn get_user(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    // We can safely unwrap here since AuthMiddleware checked all the errors before
    let token = req.token().expect("ISE: token not verified during AuthMiddleware stage");
    req.state().db
        .send(db::GetUser(token))
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
    // We can safely unwrap here since AuthMiddleware checked all the errors before
    let token = req.token().expect("ISE: token not verified during AuthMiddleware stage");
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |body: http::UpdateUser| {
            db.send(db::UpdateUser::new(token, body))
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

pub fn start(config: Config, db_addr: Addr<DbExecutor>) -> Result<Addr<Server>, Error> {
    let server = server::new(move || {
        App::with_state(AppState { db: db_addr.clone() })
            .middleware(middleware::Logger::default())
            .prefix("/1")
            .resource("/register", |r| r.method(Method::POST).a(register))
            .resource("/login", |r| r.method(Method::POST).a(login))
            .resource("/summary", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_summary);
            })
            .resource("/state", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_state);
            })
            .resource("/settings", |r| {
                r.middleware(AuthMiddleware);
                r.method(Method::GET).a(get_settings);
                r.method(Method::POST).a(update_settings);
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
    db: Addr<DbExecutor>
}

struct AuthMiddleware;
impl Middleware<AppState> for AuthMiddleware {
    fn start(&self, req: &HttpRequest<AppState>) -> actix_web::Result<Started> {
        use std::str::FromStr;

        // Don't touch options requests
        if req.method() == "OPTIONS" {
            return Ok(Started::Done);
        }

        let token = req.token()?;
        Ok(verify_token(token, req.state())?)
    }
}

fn verify_token(token: Uuid, state: &AppState) -> actix_web::Result<Started> {
    let user = state.db
        .send(db::GetUser(token))
        .from_err()
        .and_then(|res| match res {
            Ok(user) => Ok(None),
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
