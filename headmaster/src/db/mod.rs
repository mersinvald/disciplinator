pub mod models;
pub mod schema;

use self::models::{FitbitCredentials, NewUser, Settings, SummaryCache, Token, User};

use diesel::prelude::*;

use crate::proto::http as proto_http;
use crate::proto::Error as ServiceError;
use actix_web::actix::{Actor, Handler, Message, SyncContext};
use chrono::{NaiveDate, Utc};
use diesel::r2d2::ConnectionManager;
use diesel::PgConnection;
use failure::Error;
use log::debug;
use r2d2::Pool;
use uuid::Uuid;

use actix_web::Json;

/// This is db executor actor. We are going to run 3 of them in parallel.
pub struct DbExecutor(pub Pool<ConnectionManager<PgConnection>>);

pub struct CreateUser {
    pub username: String,
    pub email: String,
    pub passwd_hash: Vec<u8>,
}

impl CreateUser {
    pub fn from_body(body: Json<proto_http::Register>) -> Self {
        let body = body.into_inner();
        let passwd_hash = crate::util::sha256hash(body.passwd.as_bytes());
        CreateUser {
            username: body.username,
            email: body.email,
            passwd_hash,
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

    #[allow(clippy::len_zero)]
    fn handle(&mut self, msg: CreateUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users;
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        // Check that there's no user with the same username
        let username_exists = users
            .filter(username.eq(&msg.username))
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
        let email_exists = users
            .filter(email.eq(&msg.email))
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
            email_verified: false,
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

impl LoginUser {
    pub fn from_body(body: Json<proto_http::Login>) -> Self {
        let body = body.into_inner();
        let passwd_hash = crate::util::sha256hash(body.passwd.as_bytes());
        LoginUser {
            username: body.username,
            passwd_hash,
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

        let fetched_user = users
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
    update: proto_http::UpdateUser,
}

impl UpdateUser {
    pub fn new(user_id: i64, update: proto_http::UpdateUser) -> Self {
        UpdateUser { user_id, update }
    }

    pub fn from_json(user_id: i64, update: Json<proto_http::UpdateUser>) -> Self {
        Self::new(user_id, update.into_inner())
    }
}

impl Message for UpdateUser {
    type Result = Result<User, Error>;
}

impl Handler<UpdateUser> for DbExecutor {
    type Result = Result<User, Error>;

    fn handle(&mut self, msg: UpdateUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users::dsl::*;

        let conn = self.0.get()?;

        // Check that there is user with provided old_passwd
        let new_passwd_hash = if let Some(old_passwd) = msg.update.old_passwd {
            let old_passwd_hash = crate::util::sha256hash(old_passwd.as_bytes());

            let _ = users
                .filter(id.eq(&msg.user_id))
                .filter(passwd_hash.eq(&old_passwd_hash))
                .first::<User>(&conn)
                .map_err(|_| ServiceError::UserNotFound)?;

            msg.update
                .new_passwd
                .map(|p| crate::util::sha256hash(p.as_bytes()))
        } else {
            None
        };

        let changeset = models::UpdateUser {
            username: msg.update.username,
            email: msg.update.email,
            // TODO check if email have really changed
            email_verified: Some(false),
            passwd_hash: new_passwd_hash,
        };

        let updated_user = diesel::update(users)
            .filter(id.eq(msg.user_id))
            .set(changeset)
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

    fn handle(&mut self, msg: GetSettings, _: &mut Self::Context) -> Self::Result {
        use self::schema::settings::dsl::*;

        let conn = self.0.get()?;

        let mut s = settings.filter(user_id.eq(msg.0)).load::<Settings>(&conn)?;

        if s.is_empty() {
            let keys = ["hourly_activity_goal", "day_starts_at", "dat_ends_at"];
            Err(ServiceError::MissingConfig {
                keys: keys.iter().map(|s| s.to_string()).collect(),
            }
            .into())
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
    pub fn new(user_id: i64, update: Json<models::UpdateSettings>) -> Self {
        UpdateSettings {
            user_id,
            changeset: update.into_inner(),
        }
    }
}

impl Message for UpdateSettings {
    type Result = Result<Settings, Error>;
}

impl Handler<UpdateSettings> for DbExecutor {
    type Result = Result<Settings, Error>;

    fn handle(&mut self, msg: UpdateSettings, _: &mut Self::Context) -> Self::Result {
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
                if msg.changeset.hourly_activity_goal.is_none() {
                    keys.push("hourly_activity_goal".into())
                }
                if msg.changeset.day_starts_at.is_none() {
                    keys.push("day_starts_at".into())
                }
                if msg.changeset.day_ends_at.is_none() {
                    keys.push("dat_ends_at".into())
                }
                return Err(ServiceError::MissingConfig { keys }.into());
            }
        }

        let mut transaction_error = ServiceError::Internal {
            error: "uninitialized result".into(),
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
                        day_length: msg
                            .changeset
                            .day_length
                            .map(|i| if i == 0 { None } else { Some(i) })
                            .unwrap_or(None),
                        hourly_debt_limit: msg
                            .changeset
                            .hourly_debt_limit
                            .map(|i| if i == 0 { None } else { Some(i) })
                            .unwrap_or(None),
                        hourly_activity_limit: msg
                            .changeset
                            .hourly_activity_limit
                            .map(|i| if i == 0 { None } else { Some(i) })
                            .unwrap_or(None),
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
                    hint: "0 < value <= 60".into(),
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

    fn handle(&mut self, msg: GetSettingsFitbit, _: &mut Self::Context) -> Self::Result {
        use self::schema::fitbit::dsl::*;

        let conn = self.0.get()?;

        let mut s = fitbit
            .filter(user_id.eq(msg.0))
            .load::<FitbitCredentials>(&conn)?;

        if s.is_empty() {
            let keys = ["client_id", "client_secret"];
            Err(ServiceError::MissingConfig {
                keys: keys.iter().map(|s| s.to_string()).collect(),
            }
            .into())
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
    pub fn new(user_id: i64, update: models::UpdateFitbitCredentials) -> Self {
        UpdateSettingsFitbit {
            user_id,
            changeset: update,
        }
    }

    pub fn from_json(user_id: i64, update: Json<models::UpdateFitbitCredentials>) -> Self {
        Self::new(user_id, update.into_inner())
    }
}

impl Message for UpdateSettingsFitbit {
    type Result = Result<FitbitCredentials, Error>;
}

impl Handler<UpdateSettingsFitbit> for DbExecutor {
    type Result = Result<FitbitCredentials, Error>;

    fn handle(&mut self, msg: UpdateSettingsFitbit, _: &mut Self::Context) -> Self::Result {
        use self::schema::fitbit::dsl::*;

        let conn = self.0.get()?;

        // Check if settings are null at the moment
        let first_update = fitbit
            .filter(user_id.eq(msg.user_id))
            .count()
            .first::<i64>(&conn)? == 0;

        // If so -- check that all NOT NULL fields are present in the update
        if first_update {
            let all_present =
                msg.changeset.client_id.is_some() && msg.changeset.client_secret.is_some();
            // If not -- return error with missing keys list
            if !all_present {
                let mut keys = vec![];
                if msg.changeset.client_id.is_none() {
                    keys.push("client_id".into())
                }
                if msg.changeset.client_secret.is_none() {
                    keys.push("client_secret".into())
                }
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
                    client_token: msg.changeset.client_token,
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

pub struct GetCachedFitbitResponse(pub i64);

impl Message for GetCachedFitbitResponse {
    type Result = Result<Option<String>, Error>;
}

impl Handler<GetCachedFitbitResponse> for DbExecutor {
    type Result = Result<Option<String>, Error>;
    fn handle(&mut self, msg: GetCachedFitbitResponse, _: &mut Self::Context) -> Self::Result {
        use self::schema::summary_cache::dsl::*;

        let conn = self.0.get()?;

        let current_timestamp = Utc::now();

        let invalidation_lower_bound =
            match current_timestamp.checked_sub_signed(chrono::Duration::minutes(1)) {
                Some(time) => time,
                None => return Ok(None),
            };

        let cached_entity = summary_cache
            .filter(user_id.eq(msg.0))
            .filter(created_at.gt(invalidation_lower_bound))
            .limit(1)
            .get_result(&conn)
            .ok()
            .map(|e: SummaryCache| e.summary);

        Ok(cached_entity)
    }
}

pub struct PutCachedFitbitResponse(pub i64, pub String);

impl Message for PutCachedFitbitResponse {
    type Result = Result<(), Error>;
}

impl Handler<PutCachedFitbitResponse> for DbExecutor {
    type Result = Result<(), Error>;
    fn handle(&mut self, msg: PutCachedFitbitResponse, _: &mut Self::Context) -> Self::Result {
        use self::schema::summary_cache::dsl::*;

        let conn = self.0.get()?;

        let current_timestamp = Utc::now();

        diesel::insert_into(summary_cache)
            .values(SummaryCache {
                user_id: msg.0,
                created_at: current_timestamp,
                summary: msg.1,
            })
            .execute(&conn)?;

        Ok(())
    }
}

pub struct GetActiveHoursOverrides(pub i64, pub NaiveDate);

impl Message for GetActiveHoursOverrides {
    type Result = Result<Vec<proto_http::ActivityOverride>, Error>;
}

impl Handler<GetActiveHoursOverrides> for DbExecutor {
    type Result = Result<Vec<proto_http::ActivityOverride>, Error>;

    fn handle(&mut self, msg: GetActiveHoursOverrides, _: &mut Self::Context) -> Self::Result {
        use self::schema::active_hours_overrides::dsl::*;

        let conn = self.0.get()?;

        let rows = active_hours_overrides
            .filter(user_id.eq(msg.0))
            .filter(override_date.eq(msg.1))
            .select((override_hour, is_active))
            .get_results::<(i32, bool)>(&conn)?
            .into_iter()
            .map(|(hour, status)| proto_http::ActivityOverride {
                hour: hour as u32,
                is_active: status,
            })
            .collect();

        Ok(rows)
    }
}

pub struct SetActiveHoursOverrides {
    pub user_id: i64,
    pub date: NaiveDate,
    pub overrides: Vec<proto_http::ActivityOverride>,
}

impl Message for SetActiveHoursOverrides {
    type Result = Result<(), Error>;
}

impl Handler<SetActiveHoursOverrides> for DbExecutor {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: SetActiveHoursOverrides, _: &mut Self::Context) -> Self::Result {
        use self::schema::active_hours_overrides::dsl::*;

        let conn = self.0.get()?;

        for o in msg.overrides {
            diesel::insert_into(active_hours_overrides)
                .values(models::ActiveHoursOverrides {
                    user_id: msg.user_id,
                    override_date: msg.date,
                    override_hour: o.hour as i32,
                    is_active: o.is_active,
                })
                .on_conflict((user_id, override_date, override_hour))
                .do_update()
                .set(is_active.eq(o.is_active))
                .execute(&conn)?;
        }

        Ok(())
    }
}
