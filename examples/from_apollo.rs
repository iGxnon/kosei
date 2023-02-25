use kosei::{Config, ConfigType};
use serde::Deserialize;
use kosei::apollo::Builder;

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
    let entry = Config::<Entry>::from_apollo(&client)
        .await
        .unwrap()
        .into_inner();
    println!("entry: {:?}", entry);
}
