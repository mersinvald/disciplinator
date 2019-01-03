use failure::{format_err, Error};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use headmaster::{CurrentHourSummary, State};

pub type Callback = Box<dyn Fn(State)>;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum CallbackTrigger {
    Normal,
    DebtCollection,
    DebtCollectionPaused,
}

impl CallbackTrigger {
    fn is_triggered_for(self, state: &State) -> bool {
        match self {
            CallbackTrigger::Normal => match state {
                State::Normal => true,
                _ => false,
            },
            CallbackTrigger::DebtCollection => match state {
                State::DebtCollection(..) => true,
                _ => false,
            },
            CallbackTrigger::DebtCollectionPaused => match state {
                State::DebtCollectionPaused(..) => true,
                _ => false,
            },
        }
    }
}

pub struct Driver {
    url: String,
    period: Duration,
    callbacks: Vec<(CallbackTrigger, Callback)>,
}

impl Driver {
    pub fn new<A: AsRef<str>>(url: A, period: Duration) -> Self {
        let url = url.as_ref().to_owned();
        Driver {
            url,
            period,
            callbacks: vec![],
        }
    }

    pub fn add_callback(&mut self, trigger: CallbackTrigger, callback: Callback) {
        self.callbacks.push((trigger, callback));
        debug!("registered callback for {:?}", trigger);
    }

    pub fn run(self) {
        loop {
            info!("starting update");
            match self.do_iteration() {
                Ok(_) => info!("update finished"),
                Err(e) => error!("uodate failed: {}", e),
            }
            std::thread::sleep(self.period);
        }
    }

    fn do_iteration(&self) -> Result<(), Error> {
        debug!("querying {}", self.url);
        let response = reqwest::get(&self.url)
            .map_err(|e| format_err!("failed to GET {}: {}", self.url, e))?;

        let state = serde_json::from_reader(response)
            .map_err(|e| format_err!("failed to deserialize response: {}", e))?;
        info!("current state is {:?}", state);

        self.callbacks
            .iter()
            .filter(|(trigger, _)| trigger.is_triggered_for(&state))
            .inspect(|(trigger, _)| info!("triggering callback for event {:?}", trigger))
            .for_each(|(_, callback)| callback(state));

        Ok(())
    }
}
