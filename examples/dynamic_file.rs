use kosei::{DynamicConfig, InnerWatcher};
use serde::Deserialize;
use std::time::Duration;
use tokio::fs::{read_to_string, write};

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

fn swap(before: String) -> String {
    if before == "x = 1919.810\ny = 11.4514" {
        "x = 11.4514\ny = 1919.810".to_string()
    } else {
        "x = 1919.810\ny = 11.4514".to_string()
    }
}

#[tokio::main]
async fn main() {
    let (config, mut watcher) = DynamicConfig::<Entry>::watch_file("examples/config.toml");
    watcher.verbose();
    watcher.watch().unwrap();
    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }

    // file modification event
    let before = read_to_string("examples/config.toml").await.unwrap();
    write("examples/config.toml", swap(before)).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await; // delay for a second

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }

    watcher.stop().unwrap();
}
