use kosei::{nacos::Builder, Config, ConfigType};
use serde::Deserialize;

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
    let entry = Config::<Entry>::from_nacos(&client)
        .await
        .unwrap()
        .into_inner();
    println!("entry: {:?}", entry);
}
