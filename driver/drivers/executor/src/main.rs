use driver::{CallbackTrigger, Driver, State};
use failure::{format_err, Error};
use itertools::Itertools;
use log::{debug, error, info, trace, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use structopt::StructOpt;

#[derive(Clone, Debug, StructOpt)]
#[structopt(
    name = "executor-driver",
    about = "Driver that launches scripts and binaries (plugins) from the provided folder on Headmaster events"
)]
struct Options {
    /// Headmaster state querying period (in seconds)
    #[structopt(short = "p", long = "period", default_value = "60")]
    period: u64,

    /// Plugins directory path, default = "./plugins"
    #[structopt(
        short = "d",
        long = "plugins-dir",
        default_value = "./plugins",
        parse(from_os_str)
    )]
    plugins: PathBuf,

    /// Headmaster Url
    url: String,
}

#[derive(Debug, Clone)]
struct Plugin {
    trigger: CallbackTrigger,
    path: PathBuf,
}

fn discover_plugins<P: AsRef<Path>>(base_dir: P) -> Result<Vec<Plugin>, Error> {
    use std::fs::{self, DirEntry};
    use std::io::Read;

    #[derive(Deserialize)]
    struct TriggerList {
        triggers: Vec<CallbackTrigger>,
    }

    let base_dir = base_dir.as_ref();
    debug!("discovering plugins in {}", base_dir.display());
    if !base_dir.exists() {
        return Err(format_err!(
            "plugins directory {} not found",
            base_dir.display()
        ));
    }

    let dir = fs::read_dir(base_dir)
        .map_err(|e| format_err!("failed to open plugins directory: {}", e))?;

    trace!("discovering toml manifest files");
    let toml_file_paths = dir
        .flat_map(|r| match r {
            Ok(entry) => Some(entry),
            Err(e) => {
                warn!(
                    "failed to read item in the directory {}: {}",
                    base_dir.display(),
                    e
                );
                None
            }
        })
        .map(|d| d.path().to_path_buf())
        .filter(|path| path.extension().map(|e| e == "toml").unwrap_or(false));

    let mut plugins = Vec::new();
    for toml_file_path in toml_file_paths {
        let toml = toml_file_path
            .to_str()
            .expect("non-unicode paths are not supported");
        let plugin = toml.trim_right_matches(".toml");

        debug!("probing manifest({}) and plugin({})", toml, plugin);

        // This block may throw errors, but we don't want to trow from the function
        // in-place lambda will catch 'em all!
        let result = || -> Result<TriggerList, Error> {
            let mut toml_file = fs::File::open(toml)?;
            let mut contents = String::new();
            toml_file.read_to_string(&mut contents)?;
            Ok(toml::from_str(&contents)?)
        }();

        let trigger_list = match result {
            Ok(trigger_list) => trigger_list,
            Err(e) => {
                warn!("failed to process plugin manifest at {}: {}", toml, e);
                continue;
            }
        };

        // Check that plugin file exists
        if !Path::new(plugin).exists() {
            warn!(
                "plugin manifest found at {}, but there's no plugin file at {}",
                toml, plugin
            );
            continue;
        }

        for trigger in trigger_list.triggers {
            plugins.push(Plugin {
                trigger,
                path: PathBuf::from(plugin),
            })
        }
    }

    Ok(plugins)
}

fn execute_plugins(plugins: &[PathBuf], state: State) {
    for plugin in plugins {
        match execute_plugin(&plugin, state) {
            Ok(exit_code) => {
                if exit_code.success() {
                    info!("plugin {} finished", plugin.display())
                } else {
                    error!("plugin {} errored: {:?}", plugin.display(), exit_code)
                }
            }
            Err(e) => error!("failed to launch plugin {}: {}", plugin.display(), e),
        }
    }
}

fn execute_plugin(plugin: &Path, state: State) -> Result<std::process::ExitStatus, Error> {
    use std::process::Command;

    let (discriminant, active, debt) = match state {
        State::Normal => ("Normal", "".into(), "".into()),
        State::DebtCollection(stat) => (
            "DebtCollection",
            format!("{}", stat.active_minutes),
            format!("{}", stat.debt),
        ),
        State::DebtCollectionPaused(stat) => (
            "DebtCollectionPaused",
            format!("{}", stat.active_minutes),
            format!("{}", stat.debt),
        ),
    };

    let status = Command::new(plugin)
        .args(&[discriminant, &active, &debt])
        .status()?;

    Ok(status)
}

fn register_callback(driver: &mut Driver, trigger: CallbackTrigger, plugins: Vec<PathBuf>) {
    driver.add_callback(
        trigger,
        Box::new(move |state| execute_plugins(&plugins, state)),
    );
}

fn main() {
    let options = Options::from_args();
    env_logger::init();

    let plugins = match discover_plugins(&options.plugins) {
        Ok(plugins) => plugins,
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    };

    let mut plugins = plugins
        .into_iter()
        .map(|p| (p.trigger, p.path))
        .into_group_map();

    let mut driver = Driver::new(&options.url, Duration::from_secs(options.period));

    register_callback(
        &mut driver,
        CallbackTrigger::Normal,
        plugins.remove(&CallbackTrigger::Normal).unwrap(),
    );
    register_callback(
        &mut driver,
        CallbackTrigger::DebtCollection,
        plugins.remove(&CallbackTrigger::DebtCollection).unwrap(),
    );
    register_callback(
        &mut driver,
        CallbackTrigger::DebtCollectionPaused,
        plugins
            .remove(&CallbackTrigger::DebtCollectionPaused)
            .unwrap(),
    );

    driver.run();
}
