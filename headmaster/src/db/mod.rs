pub mod schema;
pub mod models;

use self::models::{User, NewUser, Token};

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
use uuid::Uuid;
use log::{debug, info};

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
    type Result = Result<Uuid, Error>;
}

impl Handler<LoginUser> for DbExecutor {
    type Result = Result<Uuid, Error>;

    fn handle(&mut self, msg: LoginUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::tokens;
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        debug!("fetching user for login {}", msg.username);

        let mut fetched_user = users
            .filter(username.eq(&msg.username))
            .filter(passwd_hash.eq(&msg.passwd_hash))
            .first::<User>(&conn)
            .map_err(|_| ServiceError::UserNotFound)?;

        debug!("user {} found: id({})", msg.username, fetched_user.id);

        // Remove all previous tokens of this user
        diesel::delete(tokens::table)
            .filter(tokens::dsl::user_id.eq(fetched_user.id))
            .execute(&conn)?;

        // Insert new token
        let token = Uuid::new_v4();
        let token = Token {
            user_id: fetched_user.id,
            token,
        };

        let token = diesel::insert_into(tokens::table)
            .values(&token)
            .get_result::<Token>(&conn)?;

        // Return token-uuid
        Ok(token.token)
    }
}

pub struct GetUser(pub Uuid);

impl Message for GetUser {
    type Result = Result<User, Error>;
}

impl Handler<GetUser> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: GetUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::tokens::dsl::*;
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        let auth_user_id = tokens
            .filter(token.eq(&msg.0))
            .select(user_id)
            .single_value();

        let auth_user = users
            .filter(id.nullable().eq(auth_user_id))
            .first::<User>(&conn)
            .map_err(|_| ServiceError::UserNotFound)?;

        Ok(auth_user)
    }
}


pub struct UpdateUser {
    token: Uuid,
    changeset: models::UpdateUser,
}

impl UpdateUser {
    pub fn new(token: Uuid, update: proto_http::UpdateUser) -> Self {
        let email_verified = update.email.as_ref().map(|_| true);
        UpdateUser {
            token,
            changeset: models::UpdateUser {
                username: update.username,
                email: update.email,
                email_verified,
                passwd_hash: update.passwd.map(|p| crate::util::sha256hash(p.as_bytes()))
            }
        }
    }
}

impl Message for UpdateUser {
    type Result = Result<User, Error>;
}

impl Handler<UpdateUser> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: UpdateUser, c: &mut Self::Context) -> Self::Result {
        use self::schema::users::dsl::*;

        let target_user = self.handle(GetUser(msg.token), c)?;

        let conn = self.0.get()?;

        let updated_user = diesel::update(users)
            .filter(id.eq(target_user.id))
            .set(msg.changeset)
            .get_result(&conn)?;

        Ok(updated_user)
    }
}


