use kosei::apollo::Builder;
use kosei::ConfigType;

#[tokio::main]
async fn main() {
    let client = Builder::new()
        .app_id("test")
        .namespace("test", ConfigType::YAML)
        .server_url("http://localhost:8080")
        .finish();

    println!("no cache fetch: {:?}", client.fetch().await);
    println!("cache fetch: {:?}", client.cached_fetch().await);
}
