use kosei::{ApolloClient, ConfigType, DynamicConfig, InnerWatcher, WatchMode};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

#[tokio::main]
async fn main() {
    let client = ApolloClient::new("http://localhost:8080")
        .appid("test")
        .namespace("test", ConfigType::YAML);
    let (config, mut watcher) =
        DynamicConfig::<Entry>::watch_apollo(client, WatchMode::RealTime).await;

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
