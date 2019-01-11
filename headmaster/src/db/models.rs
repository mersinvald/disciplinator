use crate::db::schema::*;
use diesel::{Queryable, Insertable};
use chrono::NaiveTime;

#[derive(Queryable)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub email_verified: bool,
    pub passwd_hash: Vec<u8>,
}

#[derive(Insertable)]
#[table_name = "users"]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub email_verified: bool,
    pub passwd_hash: Vec<u8>,
}

#[derive(AsChangeset, Default, Debug)]
#[table_name = "users"]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub passwd_hash: Option<Vec<u8>>,
}

#[derive(Queryable, Insertable)]
#[table_name = "config"]
pub struct Config {
    pub user_id: i64,
    pub version: i32,
    pub hourly_activity_goal: i32,
    pub day_starts_at: NaiveTime,
    pub day_ends_at: NaiveTime,
    pub day_length: Option<i32>,
    pub hourly_debt_limit: Option<i32>,
    pub hourly_activity_limit: Option<i32>,
}

#[derive(AsChangeset, Debug, Default)]
#[table_name = "config"]
struct UpdateConfig {
    pub hourly_activity_goal: Option<i32>,
    pub day_starts_at: Option<NaiveTime>,
    pub day_ends_at: Option<NaiveTime>,
    pub day_length: Option<Option<i32>>,
    pub hourly_debt_limit: Option<Option<i32>>,
    pub hourly_activity_limit: Option<Option<i32>>,
}


#[derive(Queryable, Insertable)]
#[table_name = "fitbit"]
pub struct FitbitCredentials {
    pub user_id: i64,
    pub client_id: String,
    pub client_secret: String,
    pub client_token: Option<String>,
}

#[derive(AsChangeset, Debug, Default)]
#[table_name = "fitbit"]
struct UpdateFitbitCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_token: Option<Option<String>>,
}

