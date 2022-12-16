mod apollo;
mod dynamic;
mod nacos;
#[cfg(test)]
mod test_data;

pub use apollo::*;
pub use dynamic::*;
pub use nacos::*;
#[cfg(test)]
pub use test_data::*;

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, info, trace};

const DEFAULT_BUFFER_SIZE: usize = 32;

type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Config<T>(T);

#[derive(Clone)]
pub(crate) struct Raw {
    raw_str: String,
    typ: ConfigType,
}

impl<T> Config<T>
    where
        T: Clone,
{
    pub fn as_inner(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn to_inner(&self) -> T {
        self.0.clone()
    }
}

#[derive(Clone, Copy)]
pub enum ConfigType {
    TOML,
    YAML,
    JSON,
}

fn parse_type(path: impl AsRef<Path>) -> ConfigType {
    let path = path.as_ref();
    let sub = ["toml", "yml", "yaml", "json"]
        .into_iter()
        .find(|typ| path.to_str().unwrap().ends_with(typ));
    if sub.is_none() {
        panic!("only support yaml, toml and json");
    }
    match sub.unwrap() {
        "toml" => ConfigType::TOML,
        "json" => ConfigType::JSON,
        "yml" | "yaml" => ConfigType::YAML,
        _ => unreachable!(),
    }
}

impl<T> TryFrom<Raw> for Config<T>
    where
        T: serde::de::DeserializeOwned,
{
    type Error = Error;

    fn try_from(raw: Raw) -> Result<Self, Self::Error> {
        let config = match raw.typ {
            ConfigType::TOML => Self(toml::from_str(&raw.raw_str)?),
            ConfigType::YAML => Self(serde_yaml::from_str(&raw.raw_str)?),
            ConfigType::JSON => Self(serde_json::from_str(&raw.raw_str)?)
        };
        Ok(config)
    }
}

impl<T> Config<T>
    where
        T: serde::de::DeserializeOwned,
{
    pub fn new(raw: String, typ: ConfigType) -> Self {
        match typ {
            ConfigType::TOML => Self(toml::from_str(&raw).unwrap()),
            ConfigType::YAML => Self(serde_yaml::from_str(&raw).unwrap()),
            ConfigType::JSON => Self(serde_json::from_str(&raw).unwrap())
        }
    }

    pub fn from_file(path: impl AsRef<Path>) -> Self {
        let raw = std::fs::read_to_string(path.as_ref()).unwrap();
        Self::new(raw, parse_type(path))
    }
}

#[cfg(test)]
mod test {
    // use super::*;
    //
    // #[test]
    // fn file_test() {
    //     let _: Config<Entry> = Config::from_file("../config/config.test.yml");
    //     let _: Config<Entry> = Config::from_file("../config/config.test.toml");
    // }
    //
    // #[test]
    // fn config_test() {
    //     let _: Config<Entry> = Config::new("x: 1.0\ny: 2.0\n".to_string(), ConfigType::YAML);
    //     let _: Config<Entry> = Config::new("x = 1.0\ny = 2.0\n".to_string(), ConfigType::TOML);
    // }
}
