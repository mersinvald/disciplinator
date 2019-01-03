use failure::{format_err, Error};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use headmaster::{CurrentHourSummary, State};

pub type Callback = Box<dyn Fn(State) -> Result<(), Error>>;

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
    prev_state: Option<State>,
}

impl Driver {
    pub fn new<A: AsRef<str>>(url: A, period: Duration) -> Self {
        let url = url.as_ref().to_owned();
        Driver {
            url,
            period,
            callbacks: vec![],
            prev_state: None,
        }
    }

    pub fn add_callback(&mut self, trigger: CallbackTrigger, callback: Callback) {
        self.callbacks.push((trigger, callback));
        debug!("registered callback for {:?}", trigger);
    }

    pub fn run(mut self) {
        loop {
            info!("starting update");
            match self.do_iteration() {
                Ok(_) => info!("update finished"),
                Err(e) => error!("uodate failed: {}", e),
            }
            std::thread::sleep(self.period);
        }
    }

    fn do_iteration(&mut self) -> Result<(), Error> {
        use std::mem::discriminant;

        debug!("querying {}", self.url);
        let response = reqwest::get(&self.url)
            .map_err(|e| format_err!("failed to GET {}: {}", self.url, e))?;

        let state = serde_json::from_reader(response)
            .map_err(|e| format_err!("failed to deserialize response: {}", e))?;
        info!("current state is {:?}", state);

        if self
            .prev_state
            .map_or(false, |prev| discriminant(&prev) == discriminant(&state))
        {
            info!("state is the same, callbacks are not triggered");
            return Ok(());
        }

        self.prev_state = Some(state);

        self.callbacks
            .iter()
            .filter(|(trigger, _)| trigger.is_triggered_for(&state))
            .inspect(|(trigger, _)| info!("triggering callback for event {:?}", trigger))
            .for_each(|(_, callback)| {
                if let Err(e) = callback(state) {
                    error!("callback failed: {}", e);
                }
            });

        Ok(())
    }
}
