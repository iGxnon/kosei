use kosei::{nacos::Builder, ConfigType, DynamicConfig, InnerWatcher};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

#[tokio::main]
async fn main() {
    let client = Builder::new()
        .server_url("http://localhost:8848")
        .data_id("test")
        .credential("nacos", "nacos")
        .config_type(ConfigType::YAML)
        .finish();
    let (config, mut watcher) =
        DynamicConfig::<Entry>::watch_nacos(client, Duration::from_secs(5)).await;

    watcher.watch().unwrap();

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }

    tokio::time::sleep(Duration::from_secs(20)).await;

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }
}
