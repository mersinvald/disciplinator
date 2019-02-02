// FIXME: due to diesel improper handling of proc_macro imports
//        this is necessary to suppress warnings
#![allow(proc_macro_derive_resolution_fallback)]
#![feature(await_macro, futures_api, async_await)]
#[macro_use]
extern crate diesel;

use failure::Error;

use actix_web::actix::{Actor, SyncArbiter};

mod activity;
mod config;
mod db;
mod proto;
mod util;
mod webserver;

use crate::config::Config;
use std::path::PathBuf;
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
    let db_addr = SyncArbiter::start(config.database_pool_size as usize, move || {
        DbExecutor(pool.clone())
    });

    // Start ActivityDataGrabber
    let activity_grabber =
        activity::data_grabber::DataGrabberExecutor::new(db_addr.clone()).start();

    // Create Actix SyncArbiter for Headmaster
    let evaluator =
        activity::eval::DebtEvaluatorExecutor::new(db_addr.clone(), activity_grabber).start();

    webserver::start(config, db_addr, evaluator).expect("webserver failed");

    sys.run();

    Ok(())
}
