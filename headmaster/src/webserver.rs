use failure::{Fail};
use futures::Future;
use futures::future;

use actix_web::actix::{SyncArbiter, Addr};
use actix_web::{
    server,
    http::{Method, header},
    App,
    Error,
    HttpRequest,
    HttpResponse,
    ResponseError,
    Responder
};
use actix_net::server::Server;

use actix_web::middleware::{
    self,
    Middleware,
    Started,
};

use headmaster::proto::{HourSummary, State, Summary};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenJson,
};

use crate::config::Config;
use crate::db::DbExecutor;
use crate::proto::Error as ServiceError;
use crate::proto::Response;

fn register(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn login(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
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
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
}

fn update_user(req: &HttpRequest<AppState>) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    Box::new(future::ok(ServiceError::NotImplemented.error_response()))
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
        // Don't touch options requests
        if req.method() == "OPTIONS" {
            return Ok(Started::Done);
        }

        // Get token from headers
        let token = req.headers()
            .get("AUTHORIZATION")
            .map(|value| value.to_str().ok())
            .ok_or(ServiceError::Unauthorized)?;

        match token {
            Some(t) => {
                verify_token(&t)?;
                Ok(Started::Done)
            },
            None => Err(ServiceError::Unauthorized.into())
        }
    }
}

fn verify_token(token: &str) -> Result<(), ServiceError> {
    Ok(())
}
