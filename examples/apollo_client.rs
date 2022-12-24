use kosei::{ApolloClient, ConfigType};

#[tokio::main]
async fn main() {
    let client = ApolloClient::new("http://localhost:8080")
        .appid("test")
        .secret("e97a455f259a464982a6593ab8af0ad5")
        .namespace("test", ConfigType::TOML);

    println!("no cache fetch: {:?}", client.fetch().await);
    println!("cache fetch: {:?}", client.cached_fetch().await);
}
