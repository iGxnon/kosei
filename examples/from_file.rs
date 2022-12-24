use kosei::Config;
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
struct Entry {
    x: f64,
    y: f64,
}

fn main() {
    let config = Config::<Entry>::from_file("examples/config.toml");
    let entry = config.into_inner();
    println!("entry: {:?}", entry);
}
