use kosei::{Config, ConfigType, NacosClient};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

#[tokio::main]
async fn main() {
    let mut client = NacosClient::new("http://localhost:8848")
        .data_id("test")
        .credential("nacos", "nacos")
        .config_type(ConfigType::YAML);
    let entry = Config::<Entry>::from_nacos(&mut client)
        .await
        .unwrap()
        .into_inner();
    println!("entry: {:?}", entry);
}
