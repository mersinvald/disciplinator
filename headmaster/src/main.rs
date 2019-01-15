#![feature(await_macro, futures_api, async_await)]

#[macro_use]
extern crate diesel;

use chrono::{DateTime, Local, NaiveDate, NaiveTime, Timelike};
use failure::Error;
use log::{debug, error, info};

use actix_web::actix::{SyncArbiter, Addr};
use actix_web::{
    server,
    http::{Method, header},
    App,
    HttpRequest,
    HttpResponse,
    ResponseError,
    Responder
};

use actix_web::middleware::{
    self,
    Middleware,
    Started,
};

use headmaster::proto::{HourSummary, Status, Summary};
use priestess::{
    ActivityGrabber, FitbitActivityGrabber, FitbitAuthData, FitbitToken, SleepInterval, TokenJson
};

mod config;
mod master;
mod proto;
mod webserver;
mod db;
mod util;

use crate::config::Config;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

#[derive(Clone, Debug, StructOpt)]
#[structopt(
    name = "headmaster",
    about = "Disciplinator server-side FitBit API mediator"
)]
struct Options {
    /// Config path
    #[structopt(
        short = "c",
        long = "config",
        default_value = "./headmaster.toml",
        parse(from_os_str)
    )]
    pub config_path: PathBuf,
}

use crate::db::DbExecutor;

fn main() -> Result<(), Error> {
    if std::env::var("RUST_LOG").is_err() {
        // Init logging: info globally, debug for the app
        std::env::set_var("RUST_LOG", "headmaster=info");
    }
    env_logger::init();

    // Load args
    let options = Options::from_args();

    // Load config
    let config = Config::load(&options.config_path)?;
    println!("{}", config);

    // Connect to the database
    let manager = diesel::r2d2::ConnectionManager::new(config.database_url.clone());
    let pool = r2d2::Pool::builder()
        .max_size(config.database_pool_size)
        .build(manager)?;

    // Start the System
    let sys = actix_web::actix::System::new("disciplinator");

    // Create Actix SyncArbiter entity with out DbExecutor
    let db_addr = SyncArbiter::start(
        config.database_pool_size as usize,
        move || DbExecutor(pool.clone())
    );

    // Create Actix SyncArbiter for Headmaster
    let headmaster = SyncArbiter::start(
        // TODO: separate config entity
        config.database_pool_size as usize,
        move || master::HeadmasterExecutor
    );

    let server = webserver::start(config, db_addr, headmaster)
        .expect("webserver failed");

    sys.run();

    Ok(())
}