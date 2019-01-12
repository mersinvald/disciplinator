pub mod schema;
pub mod models;

use self::models::{User, NewUser, Token, Settings, FitbitCredentials, UpdateFitbitCredentials};

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


pub struct GetUser(pub i64);

impl Message for GetUser {
    type Result = Result<User, Error>;
}

impl Handler<GetUser> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: GetUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        let fetched = users
            .filter(id.eq(msg.0))
            .first::<User>(&conn)
            .map_err(|_| ServiceError::UserNotFound)?;

        Ok(fetched)
    }
}


pub struct GetUserByToken(pub Uuid);

impl Message for GetUserByToken {
    type Result = Result<User, Error>;
}

impl Handler<GetUserByToken> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: GetUserByToken, _: &mut Self::Context) -> Self::Result {
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
    user_id: i64,
    changeset: models::UpdateUser,
}

impl UpdateUser {
    pub fn new(user_id: i64, update: proto_http::UpdateUser) -> Self {
        let email_verified = update.email.as_ref().map(|_| true);
        UpdateUser {
            user_id,
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

        let conn = self.0.get()?;

        let updated_user = diesel::update(users)
            .filter(id.eq(msg.user_id))
            .set(msg.changeset)
            .get_result(&conn)?;

        Ok(updated_user)
    }
}

pub struct GetSettings(pub i64);

impl Message for GetSettings {
    type Result = Result<Settings, Error>;
}

impl Handler<GetSettings> for DbExecutor {
    type Result = Result<Settings, Error>;

    fn handle(&mut self, msg: GetSettings, c: &mut Self::Context) -> Self::Result {
        use self::schema::settings::dsl::*;

        let conn = self.0.get()?;

        let mut s = settings
            .filter(user_id.eq(msg.0))
            .load::<Settings>(&conn)?;

        if s.len() == 0 {
            let keys = [
                "hourly_activity_goal",
                "day_starts_at",
                "dat_ends_at"
            ];
            Err(ServiceError::MissingConfig { keys: keys.iter().map(|s| s.to_string()).collect() }.into())
        } else {
            Ok(s.remove(0))
        }
    }
}

pub struct UpdateSettings {
    user_id: i64,
    changeset: models::UpdateSettings,
}

impl UpdateSettings {
    pub fn new(user_id: i64, update: models::UpdateSettings) -> Self {
        UpdateSettings {
            user_id,
            changeset: update
        }
    }
}


impl Message for UpdateSettings {
    type Result = Result<Settings, Error>;
}

impl Handler<UpdateSettings> for DbExecutor {
    type Result = Result<Settings, Error>;

    fn handle(&mut self, msg: UpdateSettings, c: &mut Self::Context) -> Self::Result {
        use self::schema::settings::dsl::*;

        let conn = self.0.get()?;

        // Check if settings are null at the moment
        let first_update = settings
            .filter(user_id.eq(msg.user_id))
            .count()
            .first::<i64>(&conn)? == 0;

        debug!("first settings update");

        // If so -- check that all NOT NULL fields are present in the update
        if first_update {
            let all_present = msg.changeset.hourly_activity_goal.is_some()
                && msg.changeset.day_starts_at.is_some()
                && msg.changeset.day_ends_at.is_some();
            // If not -- return error with missing keys list
            if !all_present {
                let mut keys = vec![];
                if msg.changeset.hourly_activity_goal.is_none() { keys.push("hourly_activity_goal".into()) }
                if msg.changeset.day_starts_at.is_none() { keys.push("day_starts_at".into()) }
                if msg.changeset.day_ends_at.is_none() { keys.push("dat_ends_at".into()) }
                return Err(ServiceError::MissingConfig { keys }.into());
            }
        }

        let mut transaction_error = ServiceError::Internal {
            error: "uninitialized result".into(),
            backtrace: "".into()
        };

        // Perform the update in transaction
        let result = conn.transaction::<_, diesel::result::Error, _>(|| {
            let updated = if first_update {
                diesel::insert_into(settings)
                    // Options should be cleared by that moment if that's first update
                    .values(&Settings {
                        user_id: msg.user_id,
                        hourly_activity_goal: msg.changeset.hourly_activity_goal.unwrap(),
                        day_starts_at: msg.changeset.day_starts_at.unwrap(),
                        day_ends_at: msg.changeset.day_ends_at.unwrap(),
                        day_length: msg.changeset.day_length.unwrap_or(None),
                        hourly_debt_limit: msg.changeset.hourly_debt_limit.unwrap_or(None),
                        hourly_activity_limit: msg.changeset.hourly_activity_limit.unwrap_or(None),
                    })
                    .get_result(&conn)?
            } else {
                diesel::update(settings)
                    .filter(user_id.eq(msg.user_id))
                    .set(msg.changeset)
                    .get_result::<Settings>(&conn)?
            };

            // Validate settings before approving the transaction
            if updated.hourly_activity_goal <= 0 || updated.hourly_activity_goal > 60 {
                transaction_error = ServiceError::InvalidSetting {
                    key: "hourly_activity_goal".into(),
                    hint: "0 < value <= 60".into()
                };

                return Err(diesel::result::Error::RollbackTransaction);
            }

            if updated.day_starts_at > updated.day_ends_at {
                transaction_error = ServiceError::InvalidSetting {
                    key: "day_starts_at | day_ends_at".into(),
                    hint: "day should start before it ends".into(),
                };

                return Err(diesel::result::Error::RollbackTransaction);
            }

            Ok(updated)
        });

        result.map_err(|e| match e {
            // If rollback happened, we should have some meaningful error there
            diesel::result::Error::RollbackTransaction => transaction_error.into(),
            other_diesel_error => other_diesel_error.into(),
        })
    }
}


pub struct GetSettingsFitbit(pub i64);

impl Message for GetSettingsFitbit {
    type Result = Result<FitbitCredentials, Error>;
}

impl Handler<GetSettingsFitbit> for DbExecutor {
    type Result = Result<FitbitCredentials, Error>;

    fn handle(&mut self, msg: GetSettingsFitbit, c: &mut Self::Context) -> Self::Result {
        use self::schema::fitbit::dsl::*;

        let conn = self.0.get()?;

        let mut s = fitbit
            .filter(user_id.eq(msg.0))
            .load::<FitbitCredentials>(&conn)?;

        if s.len() == 0 {
            let keys = [
                "client_id",
                "client_secret",
            ];
            Err(ServiceError::MissingConfig { keys: keys.iter().map(|s| s.to_string()).collect() }.into())
        } else {
            Ok(s.remove(0))
        }
    }
}

pub struct UpdateSettingsFitbit {
    user_id: i64,
    changeset: models::UpdateFitbitCredentials,
}

impl UpdateSettingsFitbit {
    pub fn new(user_id: i64, mut update: models::UpdateFitbitCredentials) -> Self {
        // Make sure to overwrite Token with NULL if credentials are changed
        if update.client_token.is_none() && (update.client_id.is_some() || update.client_secret.is_some()) {
            debug!("fitbit credentials updated: nulling the token");
            update.client_token = Some(None)
        };

        UpdateSettingsFitbit {
            user_id,
            changeset: update
        }
    }
}

impl Message for UpdateSettingsFitbit {
    type Result = Result<FitbitCredentials, Error>;
}

impl Handler<UpdateSettingsFitbit> for DbExecutor {
    type Result = Result<FitbitCredentials, Error>;

    fn handle(&mut self, msg: UpdateSettingsFitbit, c: &mut Self::Context) -> Self::Result {
        use self::schema::fitbit::dsl::*;

        let conn = self.0.get()?;

        // Check if settings are null at the moment
        let first_update = fitbit
            .filter(user_id.eq(msg.user_id))
            .count()
            .first::<i64>(&conn)? == 0;

        // If so -- check that all NOT NULL fields are present in the update
        if first_update {
            let all_present = msg.changeset.client_id.is_some()
                && msg.changeset.client_secret.is_some();
            // If not -- return error with missing keys list
            if !all_present {
                let mut keys = vec![];
                if msg.changeset.client_id.is_none() { keys.push("client_id".into()) }
                if msg.changeset.client_secret.is_none() { keys.push("client_secret".into()) }
                return Err(ServiceError::MissingConfig { keys }.into());
            }
        }

        // Perform the update
        let updated = if first_update {
            diesel::insert_into(fitbit)
                .values(FitbitCredentials {
                    user_id: msg.user_id,
                    client_id: msg.changeset.client_id.unwrap(),
                    client_secret: msg.changeset.client_secret.unwrap(),
                    client_token: msg.changeset.client_token.unwrap_or(None),
                })
                .get_result(&conn)?
        } else {
            diesel::update(fitbit)
                .filter(user_id.eq(msg.user_id))
                .set(msg.changeset)
                .get_result(&conn)?
        };

        Ok(updated)
    }
}

