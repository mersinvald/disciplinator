use crate::db::schema::*;
use diesel::{Queryable, Insertable};
use serde::{Serialize, Deserialize, Deserializer};
use chrono::{DateTime, Utc, NaiveTime};
use uuid::Uuid;

#[derive(Queryable, Serialize, Debug, Deserialize)]
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

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "settings"]
pub struct Settings {
    pub user_id: i64,
    pub hourly_activity_goal: i32,
    pub day_starts_at: NaiveTime,
    pub day_ends_at: NaiveTime,
    pub day_length: Option<i32>,
    pub hourly_debt_limit: Option<i32>,
    pub hourly_activity_limit: Option<i32>,
}

#[derive(AsChangeset, Debug, Default, Serialize, Deserialize)]
#[table_name = "settings"]
pub struct UpdateSettings {
    pub hourly_activity_goal: Option<i32>,
    pub day_starts_at: Option<NaiveTime>,
    pub day_ends_at: Option<NaiveTime>,
    #[serde(default, deserialize_with = "some_option")]
    pub day_length: Option<Option<i32>>,
    #[serde(default, deserialize_with = "some_option")]
    pub hourly_debt_limit: Option<Option<i32>>,
    #[serde(default, deserialize_with = "some_option")]
    pub hourly_activity_limit: Option<Option<i32>>,
}


#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "fitbit"]
pub struct FitbitCredentials {
    pub user_id: i64,
    pub client_id: String,
    pub client_secret: String,
    pub client_token: Option<String>,
}

#[derive(AsChangeset, Debug, Default, Serialize, Deserialize)]
#[table_name = "fitbit"]
pub struct UpdateFitbitCredentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    #[serde(default, deserialize_with = "some_option")]
    pub client_token: Option<Option<String>>,
}

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "tokens"]
pub struct Token {
    pub token: Uuid,
    pub user_id: i64,
}

#[derive(Queryable, Insertable, Serialize, Deserialize)]
#[table_name = "summary_cache"]
pub struct SummaryCache {
    pub user_id: i64,
    pub created_at: DateTime<Utc>,
    pub summary: String,
}

fn some_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where T: Deserialize<'de>,
          D: Deserializer<'de>
{
    Option::<T>::deserialize(deserializer).map(Some)
}