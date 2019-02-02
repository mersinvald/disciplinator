use crate::db::schema::*;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use diesel::{Insertable, Queryable};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Queryable, Serialize, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub email_verified: bool,
    #[serde(skip)]
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

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "settings"]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub user_id: i64,
    pub hourly_activity_goal: i32,
    pub day_starts_at: NaiveTime,
    pub day_ends_at: NaiveTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hourly_debt_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hourly_activity_limit: Option<i32>,
}

#[derive(AsChangeset, Debug, Default, Serialize, Deserialize)]
#[table_name = "settings"]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettings {
    pub hourly_activity_goal: Option<i32>,
    pub day_starts_at: Option<NaiveTime>,
    pub day_ends_at: Option<NaiveTime>,
    pub day_length: Option<i32>,
    pub hourly_debt_limit: Option<i32>,
    pub hourly_activity_limit: Option<i32>,
}

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "fitbit"]
#[serde(rename_all = "camelCase")]
pub struct FitbitCredentials {
    pub user_id: i64,
    pub client_id: String,
    pub client_secret: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

#[derive(AsChangeset, Debug, Default, Serialize, Deserialize)]
#[table_name = "fitbit"]
#[serde(rename_all = "camelCase")]
pub struct UpdateFitbitCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_token: Option<String>,
}

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "tokens"]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub token: Uuid,
    pub user_id: i64,
}

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "summary_cache"]
#[serde(rename_all = "camelCase")]
pub struct SummaryCache {
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
    pub summary: String,
}

#[derive(Queryable, Insertable)]
#[table_name = "active_hours_overrides"]
pub struct ActiveHoursOverrides {
    pub user_id: i64,
    pub override_date: NaiveDate,
    pub override_hour: i32,
    pub is_active: bool,
}
