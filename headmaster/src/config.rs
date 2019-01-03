use chrono::NaiveTime;
use failure::{format_err, Error};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub auth: Auth,
    pub limits: Limits,
    pub day: Day,
    pub network: Network,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: Config = toml::from_str(&contents)?;

        // Check invariants
        Self::check_field_ranges(
            "limits.hourly_minimum_active_time",
            config.limits.minimum_active_time,
            5,
            60,
        )?;
        Self::check_field_ranges(
            "limits.hourly_max_accounted_active_time",
            config.limits.max_accounted_active_time,
            5,
            60,
        )?;
        Self::check_field_ranges(
            "limits.absolute_debt_limit",
            config.limits.debt_limit,
            5,
            3600,
        )?;
        Self::check_field_ranges(
            "day.day_begins_at",
            config.day.day_begins_at,
            NaiveTime::from_hms(0, 0, 0),
            config.day.day_ends_at,
        )?;
        Self::check_field_ranges(
            "day.day_ends_at",
            config.day.day_ends_at,
            config.day.day_begins_at,
            NaiveTime::from_hms(23, 59, 59),
        )?;

        Ok(config)
    }

    fn check_field_ranges<T: Ord + Display>(
        name: &str,
        field: T,
        lower: T,
        upper: T,
    ) -> Result<(), Error> {
        if field < lower || field > upper {
            Err(format_err!(
                "value of the field {:?} = {} is out of range {} <= value <= {}",
                name,
                field,
                lower,
                upper
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limits {
    pub minimum_active_time: u32,
    pub max_accounted_active_time: u32,
    pub debt_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Day {
    /// used when there's no sleep data
    pub day_begins_at: NaiveTime,
    /// used regardless of sleep data: there should be some time for leisure in the evening
    pub day_ends_at: NaiveTime,
    /// day length, used if sleep data is available (in hours)
    pub day_length: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    pub addr: String,
}
