use kosei::{ApolloClient, Config, ConfigType};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

#[tokio::main]
async fn main() {
    let client = ApolloClient::new("http://localhost:8080")
        .appid("test")
        .secret("e97a455f259a464982a6593ab8af0ad5") // add secret key
        .namespace("test", ConfigType::TOML);

    let entry = Config::<Entry>::from_apollo(&client)
        .await
        .unwrap()
        .into_inner();
    println!("entry: {:?}", entry);
}
