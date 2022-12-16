<h1 align="center">Kōsei</h1>

<p align="center"><code>こうせい</code></p>
<p align="center">A easy-to-use configuration crate with the Rust programming language.</p>

## Features

- Support `toml`,`yaml`,`json` configuration type.
- **Dynamic configuration**
  - hot reload
  - support from local file and [Apollo config](https://github.com/apolloconfig/apollo)

[![Crates.io][crates-badge]][crates-url]
[![Crates.io][crates-download]][crates-url]

[crates-badge]: https://flat.badgen.net/crates/v/kosei
[crates-download]: https://flat.badgen.net/crates/d/kosei
[crates-url]: https://crates.io/crates/kosei

## Quickstart

- `Base file config`

```rust
#[test]
fn base_test() {
  // Panic if no such file `config/config.yaml`
  let config: Config<Entry> = Config::from_file("config/config.yaml");
  let entry: &Entry = config.as_inner(); // borrowed value has the same lifetimes as config
  let entry: Entry = config.to_inner();  // clone a new Entry
  let entry: Entry = config.into_inner(); // take ownership
}
```



- `Dynamic file config`

```rust
#[tokio::test]
async fn dynamic_test() {   
  // Create a dynamic config and a watcher
  let (config, mut watcher) = DynamicConfig::<Entry>::watch_file("config/config.yaml");
  // Listen to file modify event
  watcher.watch().unwrap();
  let lock = config.lock();
  let entry: &Entry = lock.as_inner();  // borrow Entry
  let entry: Entry = lock.to_inner();  // clone a new Entry
  // let entry: Entry = lock.into_inner(); panic! cannot take the lock ownership
  let arc = config.as_arc();  // clone a new arc
  // Stop watching
  watcher.stop().unwrap();
  // You can watch twice
  watcher.watch().unwrap();
}
```



- `Dynamic Apollo config`

```rust

#[tokio::test] 
async fn apollo_test() {
  // First build a ApolloClient
  let client = ApolloClient::new("http://localhost:8080")
          .appid("114514")
          .namespace("test", ConfigType::TOML);
  // Create a watcher to fetch apollo config changes at a `RealTime` mode.
  // `Entry` indicates how data should be deserialized.
  // The returned config type is `DynamicConfig<Entry>`
  let (config, mut watcher) =
          DynamicConfig::<Entry>::watch_apollo(client, WatchMode::RealTime).await;
  // Enable verbose mode (log messages with INFO level)
  watcher.verbose();
  // Start watching
  watcher.watch().unwrap();
  
  println!("{:?}", config);
  // Stop watching
  watcher.stop().unwrap();
  
  // You can start twice even you forget to call stop()
  watcher.watch().unwrap();
  
  // All changes will be reflected to config in time
  do_somthing(config);
}

```

[crate]: 