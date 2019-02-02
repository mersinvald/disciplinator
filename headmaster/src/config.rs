use failure::Error;
use log::warn;
use serde::Deserialize;
use std::env;
use std::fmt::Display;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub database_pool_size: u32,
    pub listen_on: String,
}

impl Config {
    #[allow(clippy::or_fun_call)]
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        // send warning if env is set without prefix
        if env::var("DATABASE_URL")
            .or(env::var("DATABASE_POOL_SIZE"))
            .or(env::var("LISTEN_ON"))
            .is_ok()
        {
            warn!("use prefix HEADMASTER_ to configure through ENV variables")
        }

        let path = path.as_ref();

        // Firstly get the env config
        let env = envy::prefixed("HEADMASTER_").from_env::<EnvConfig>()?;

        let config = Self::load_file(&path)
            .map_err(|e| warn!("failed to read config file '{}': {}", path.display(), e))
            .ok();

        macro_rules! fallback_if_none {
            ($option:expr, $key:tt, $default:expr) => {
                $option.$key.unwrap_or_else(|| {
                    warn!(
                        "configuration incomplete for key '{}', using default value '{}'",
                        stringify!($key),
                        $default
                    );
                    $default.into()
                })
            };
        }

        let config = match config {
            Some(config) => Config {
                database_url: env.database_url.unwrap_or(config.database_url),
                database_pool_size: env.database_pool_size.unwrap_or(config.database_pool_size),
                listen_on: env.listen_on.unwrap_or(config.listen_on),
            },
            None => Config {
                database_url: fallback_if_none!(
                    env,
                    database_url,
                    "postgres://headmaster:headmaster@postgres/headmaster"
                ),
                database_pool_size: fallback_if_none!(env, database_pool_size, 4_u32),
                listen_on: fallback_if_none!(env, listen_on, "127.0.0.1:8080"),
            },
        };

        Ok(config)
    }

    fn load_file<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config = toml::from_str(&contents)?;
        Ok(config)
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Running with configuration: ")?;
        writeln!(f, "  database_url: {}", self.database_url)?;
        writeln!(f, "  pool_size:    {}", self.database_pool_size)?;
        write!(f, "  listen_on:    {}", self.listen_on)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvConfig {
    database_url: Option<String>,
    database_pool_size: Option<u32>,
    listen_on: Option<String>,
}
