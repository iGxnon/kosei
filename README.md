<h1 align="center">Kōsei</h1>

<p align="center"><code>こうせい</code></p>
<p align="center">A easy-to-use configuration crate with the Rust programming language.</p>

## Features

- **Supports**:
    - `toml`, `yaml`, `json` configuration type.
    - [Apollo config](https://github.com/apolloconfig/apollo)
    - [Nacos](https://nacos.io/zh-cn/)

- **Dynamic configuration**
    - hot reload
    - support from local file, [Apollo config](https://github.com/apolloconfig/apollo), [Nacos](https://nacos.io/zh-cn/)

[![Crates.io][crates-badge]][crates-url]
[![Crates.io][crates-download]][crates-url]

[crates-badge]: https://flat.badgen.net/crates/v/kosei

[crates-download]: https://flat.badgen.net/crates/d/kosei

[crates-url]: https://crates.io/crates/kosei

## Features

| dynamic    | hot-reload config support |
| ---------- | ------------------------- |
| **apollo** | **Apollo support**        |
| **nacos**  | **Nacos support**         |

## Quickstart

> See [`examples`](examples) for further use.

Config Entry

```rust
// `Deserialize` and `Clone` traits should be applied
#[derive(Clone, Debug, Deserialize)]
struct Entry {
    ...
}
```

- **Base file config**

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

- **Dynamic file config**

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

- **Dynamic Apollo config**

```rust
use kosei::apollo::{Builder, WatchMode};
use kosei::{ConfigType, DynamicConfig, InnerWatcher};
use serde::Deserialize;
use std::time::Duration;

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
    let (config, mut watcher) =
        DynamicConfig::<Entry>::watch_apollo(client, WatchMode::RealTime).await;

    watcher.watch().unwrap();

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }

    tokio::time::sleep(Duration::from_secs(10)).await;

    {
        let guard = config.lock();
        println!("entry: {:?}", guard.as_inner());
    }
}


```
