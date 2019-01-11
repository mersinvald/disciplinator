pub mod schema;
pub mod models;

use self::models::{User, NewUser};

use diesel::prelude::*;
use diesel::associations::*;

use actix_web::actix::{Message, Actor, SyncContext, Handler};
use r2d2::Pool;
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;
use std::io;
use failure::{Fail, Error};
use crate::proto::Error as ServiceError;
use crate::proto::http as proto_http;
use sha2::{Sha256, Digest};

/// This is db executor actor. We are going to run 3 of them in parallel.
pub struct DbExecutor(pub Pool<ConnectionManager<PgConnection>>);

pub struct CreateUser {
    pub username: String,
    pub email: String,
    pub passwd_hash: Vec<u8>,
}

impl From<proto_http::Register> for CreateUser {
    fn from(body: proto_http::Register) -> Self {
        CreateUser {
            username: body.username,
            email: body.email,
            passwd_hash: crate::util::sha256hash(body.passwd.as_bytes())
        }
    }
}

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

impl Message for CreateUser {
    type Result = Result<i64, Error>;
}

impl Handler<CreateUser> for DbExecutor {
    type Result = Result<i64, Error>;

    fn handle(&mut self, msg: CreateUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users;
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        // Check that there's no user with the same username
        let username_exists = users.filter(username.eq(&msg.username))
            .limit(1)
            .load::<User>(&conn)?
            .len() != 0;

        if username_exists {
            return Err(ServiceError::CredentialsConflict {
                key: "username".into(),
                value: msg.username.clone()
            }.into());
        }

        // Check that there's no user with the same email
        let email_exists = users.filter(email.eq(&msg.email))
            .limit(1)
            .load::<User>(&conn)?
            .len() != 0;

        if email_exists {
            return Err(ServiceError::CredentialsConflict {
                key: "email".into(),
                value: msg.email.clone()
            }.into());
        }

        // Insert new user
        let new_user = NewUser {
            username: msg.username,
            email: msg.email,
            passwd_hash: msg.passwd_hash,
            email_verified: false
        };

        let user = diesel::insert_into(users::table)
            .values(&new_user)
            .get_result::<User>(&conn)?;

        // Return user id
        Ok(user.id)
    }
}

pub struct LoginUser {
    pub username: String,
    pub passwd_hash: Vec<u8>,
}

impl From<proto_http::Login> for LoginUser {
    fn from(body: proto_http::Login) -> Self {
        LoginUser {
            username: body.username,
            passwd_hash: crate::util::sha256hash(body.passwd.as_bytes())
        }
    }
}

impl Message for LoginUser {
    type Result = Result<User, Error>;
}

impl Handler<LoginUser> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: LoginUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        let mut fetched_users = users
            .filter(username.eq(msg.username))
            .filter(passwd_hash.eq(msg.passwd_hash))
            .load::<User>(&conn)?;

        match fetched_users.len() {
            0 => Err(ServiceError::UserNotFound.into()),
            1 => Ok(fetched_users.remove(0)),
            _ => panic!("more then 1 user with the same login:passwd pair")
        }
    }
}
