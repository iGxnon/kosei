#[cfg(feature = "apollo")]
mod apollo;
#[cfg(feature = "dynamic")]
mod dynamic;
mod nacos;
#[cfg(test)]
mod test_data;

#[cfg(feature = "apollo")]
pub use apollo::*;
#[cfg(feature = "dynamic")]
pub use dynamic::*;
pub use nacos::*;
#[cfg(test)]
use test_data::*;

use std::path::Path;

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
            ConfigType::JSON => Self(serde_json::from_str(&raw.raw_str)?),
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
            ConfigType::JSON => Self(serde_json::from_str(&raw).unwrap()),
        }
    }

    pub fn from_file(path: impl AsRef<Path>) -> Self {
        let raw = std::fs::read_to_string(path.as_ref()).unwrap();
        Self::new(raw, parse_type(path))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn config_test() {
        let _: Config<Entry> = Config::new(ENTRY_RAW_YML.to_string(), ConfigType::YAML);
        let _: Config<Entry> = Config::new(ENTRY_RAW_TOML.to_string(), ConfigType::TOML);
        let _: Config<Entry> = Config::new(ENTRY_RAW_JSON.to_string(), ConfigType::JSON);
    }
    //
    // #[test]
    // fn base_test() {
    //     // panic if no such file `config/config.yaml`
    //     let config: Config<Entry> = Config::from_file("config/config.yaml");
    //     let entry: &Entry = config.as_inner(); // borrowed value has the same lifetimes as config
    //     let entry: Entry = config.to_inner(); // clone a new Entry
    //     let entry: Entry = config.into_inner(); // take ownership
    // }
    //
    // #[tokio::test]
    // async fn dynamic_test() {
    //     // Create a dynamic config and a watcher
    //     let (config, mut watcher) = DynamicConfig::<Entry>::watch_file("config/config.yaml");
    //     // Listen to file modify event
    //     watcher.watch().unwrap();
    //     let lock = config.lock();
    //     let entry: &Entry = lock.as_inner();  // borrow Entry
    //     let entry: Entry = lock.to_inner();  // clone a new Entry
    //     // let entry: Entry = lock.into_inner(); panic! cannot take the lock ownership
    //     let arc = config.as_arc();  // clone a new arc
    //     // Stop watching
    //     watcher.stop().unwrap();
    //     // You can watch twice
    //     watcher.watch().unwrap();
    // }
}
