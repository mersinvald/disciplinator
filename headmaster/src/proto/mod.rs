pub mod activity;
pub mod http;


use failure::{Fail, AsFail};
pub use self::activity::{HourSummary, State, Summary};
use serde::{Serialize, Deserialize, de::DeserializeOwned};
use actix_web::actix::MailboxError;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response<D, E> {
    data: Option<D>,
    error: Option<ErrorBody<E>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody<E> {
    #[serde(flatten)]
    error: E,
    message: String,
}

impl<E: std::fmt::Display> ErrorBody<E> {
    fn new(error: E) -> Self {
        let message = format!("{}", error);
        ErrorBody {
            error,
            message
        }
    }
}

impl<D> Response<D, ()> {
    pub fn data(data: D) -> Self {
        Response {
            data: Some(data),
            error: None
        }
    }
}

impl<E: std::fmt::Display> Response<(), E> {
    pub fn error(error: E) -> Self {
        Response {
            data: None,
            error: Some(ErrorBody::new(error)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Fail)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Error {
    #[fail(display = "payload is invalid")]
    InvalidPayload { error: String },
    #[fail(display = "{} {:?} already exists", key, value)]
    CredentialsConflict { key: String, value: String },
    #[fail(display = "email {:?} is not verified", email)]
    EmailNotVerified { email: String },
    #[fail(display = "value of setting {:?} is invalid. hint: {}", key, hint)]
    InvalidSetting { key: String, hint: String },
    #[fail(display = "user with provided username:password not found")]
    UserNotFound,
    #[fail(display = "missing the following configuration entries: {:?}", keys)]
    MissingConfig { keys: Vec<String> },
    #[fail(display = "token have expired")]
    TokenExpired,
    #[fail(display = "authorization required")]
    Unauthorized,
    #[fail(display = "not yet implemented")]
    NotImplemented,
    #[fail(display = "internal error")]
    Internal { error: String },
}

pub trait DataResponse: Serialize + DeserializeOwned + std::fmt::Debug + Clone {}

impl<T: DataResponse> From<T> for Response<T, ()> {
    fn from(data: T) -> Self {
        Response {
            data: Some(data),
            error: None,
        }
    }
}

impl From<failure::Error> for Error {
    fn from(error: failure::Error) -> Self {
        match error.downcast::<Error>() {
            Ok(error) => error.clone(),
            Err(error) => Error::Internal {
                error: format!("{}", error),
            }
        }
    }
}

pub trait IntoError: Sized + std::fmt::Display {
    fn into_error(self) -> Error {
        Error::Internal {
            error: format!("{}", self)
        }
    }
}

impl<E> From<E> for Error
    where E: IntoError
{
    fn from(error: E) -> Error {
        error.into_error()
    }
}

impl IntoError for MailboxError {}

use actix_web::{ResponseError, HttpResponse};

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse {
        use self::Error::*;
        let response = Response::error(self.clone());
        match self {
            InvalidPayload { .. } => HttpResponse::BadRequest().json(response),
            CredentialsConflict { .. } => HttpResponse::Conflict().json(response),
            EmailNotVerified { .. } => HttpResponse::Forbidden().json(response),
            InvalidSetting { .. } => HttpResponse::Forbidden().json(response),
            UserNotFound => HttpResponse::Unauthorized().json(response),
            MissingConfig { .. } => HttpResponse::Forbidden().json(response),
            TokenExpired => HttpResponse::Unauthorized().json(response),
            Unauthorized => HttpResponse::Unauthorized().json(response),
            NotImplemented => HttpResponse::InternalServerError().json(response),
            Internal { .. } => HttpResponse::InternalServerError().json(response),
        }
    }
}