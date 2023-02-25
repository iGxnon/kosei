use kosei::apollo::{Builder, WatchMode};
use kosei::{ConfigType, DynamicConfig, InnerWatcher};
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
        .app_id("test")
        .namespace("test", ConfigType::YAML)
        .server_url("http://localhost:8080")
        .finish();
    let (config, mut watcher) =
        DynamicConfig::<Entry>::watch_apollo(client, WatchMode::RealTime).await;

    watcher.watch().unwrap();

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }

    tokio::time::sleep(Duration::from_secs(10)).await;

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }
}
