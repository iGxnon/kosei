use super::*;

const CACHE_FETCH_THRESHOLD: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Clone)]
pub struct ApolloClient {
    server_url: String,
    appid: String,
    cluster_name: String,
    namespace_name: String,
    namespace_type: ConfigType,
    release_key: String,
    local_ip: String,
}

// releaseKey
#[derive(Debug, Default, Clone)]
pub struct Metadata(String);

pub enum WatchMode {
    RealTime,           // RealTime configuration updating (about 1s, based on Http long polling)
    Interval(Duration), // Each interval is updated, preferably greater than 30s, several hours or days are recommended
}

pub struct ApolloWatcher {
    client: ApolloClient,
    tx: broadcast::Sender<Raw>,
    mode: WatchMode,
    handle: Option<JoinHandle<()>>,
}

impl Default for ApolloClient {
    fn default() -> Self {
        Self {
            server_url: "".to_string(),
            appid: "".to_string(),
            cluster_name: "default".to_string(),
            namespace_name: "application.txt".to_string(),
            release_key: "".to_string(),
            local_ip: "".to_string(),
            namespace_type: ConfigType::TOML,
        }
    }
}

impl ApolloClient {
    pub fn new(uri: &str) -> Self {
        Self {
            server_url: uri.to_string(),
            ..Default::default()
        }
    }

    // Required
    pub fn appid(mut self, id: &str) -> Self {
        self.appid = id.to_string();
        self
    }

    // Required
    pub fn cluster(mut self, cluster: &str) -> Self {
        self.cluster_name = cluster.to_string();
        self
    }

    // Required
    // config namespace without suffix type
    // NOTICE: yaml not yml
    pub fn namespace(mut self, ns: &str, typ: ConfigType) -> Self {
        match typ {
            ConfigType::TOML => {
                self.namespace_name = format!("{}.txt", ns);
                self.namespace_type = ConfigType::TOML;
            }
            ConfigType::YAML => {
                self.namespace_name = format!("{}.yaml", ns);
                self.namespace_type = ConfigType::YAML;
            }
            ConfigType::JSON => {
                self.namespace_name = format!("{}.json", ns);
                self.namespace_type = ConfigType::JSON;
            }
        }
        self
    }

    // Optional
    pub fn release_key(mut self, key: &str) -> Self {
        self.release_key = key.to_string();
        self
    }

    // Optional
    pub fn local_id(mut self, ip: &str) -> Self {
        self.local_ip = ip.to_string();
        self
    }

    // // Only support `.properties`
    // // NOTICE: make sure namespace must be a `.properties` type
    // pub async fn fetch_properties(&self) -> Result<(HashMap<String, String>, Metadata), Error> {
    //     let resp = reqwest::get(self.nocache_url())
    //         .await?
    //         .json::<serde_json::Value>()
    //         .await?;
    //     let config = resp
    //         .get("configurations")
    //         .map(ToOwned::to_owned)
    //         .map(serde_json::from_value::<HashMap<String, String>>)
    //         .unwrap_or_else(|| Ok(HashMap::new()))?;
    //     let release_key = resp
    //         .get("releaseKey")
    //         .and_then(|v| v.as_str())
    //         .map(ToString::to_string)
    //         .unwrap_or_else(|| "".to_string());
    //     Ok((config, Metadata(release_key)))
    // }

    // Not support `.properties`
    // NOTICE: make sure namespace had been published and set to a support config type
    // (responses should contains `configurations.content`)
    pub async fn fetch(&self) -> Result<(String, Metadata), Error> {
        let resp = reqwest::get(self.nocache_url())
            .await?
            .json::<serde_json::Value>()
            .await?;
        let config = resp
            .get("configurations")
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "".to_string());
        let release_key = resp
            .get("releaseKey")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "".to_string());
        Ok((config, Metadata(release_key)))
    }

    pub async fn cached_fetch(&self) -> Result<String, Error> {
        let config = reqwest::get(self.cached_url())
            .await?
            .json::<serde_json::Value>()
            .await?
            .get("content")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "".to_string());
        Ok(config)
    }

    // fetch notification id, if not updated, returns the same as the input notify_id.
    pub async fn notification_fetch(&self, notify_id: isize) -> Result<isize, Error> {
        let resp = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(70))
            .build()?
            .get(self.notify_url(notify_id))
            .send()
            .await?;
        if resp.status().as_u16() == 304 {
            return Ok(notify_id);
        }
        let new_id = resp
            .json::<serde_json::Value>()
            .await?
            .as_array()
            .and_then(|v| v.first())
            .and_then(|v| v.get("notificationId"))
            .and_then(|v| v.as_i64())
            .map(|v| v as isize)
            .unwrap_or(notify_id);
        Ok(new_id)
    }

    fn nocache_url(&self) -> String {
        format!(
            "{server_url}/configs/{appid}/{cluster}/{namespace}?releaseKey={rk}&ip={ip}",
            server_url = self.server_url,
            appid = self.appid,
            cluster = self.cluster_name,
            namespace = self.namespace_name,
            rk = self.release_key,
            ip = self.local_ip
        )
    }

    fn cached_url(&self) -> String {
        format!(
            "{server_url}/configfiles/json/{appid}/{cluster}/{namespace}?ip={ip}",
            server_url = self.server_url,
            appid = self.appid,
            cluster = self.cluster_name,
            namespace = self.namespace_name,
            ip = self.local_ip
        )
    }

    fn notify_url(&self, notify_id: isize) -> String {
        let notify = format!(
            r#"[{{"namespaceName": "{}", "notificationId": {}}}]"#,
            self.namespace_name, notify_id
        );
        format!(
            "{server_url}/notifications/v2?appId={appid}&cluster={cluster}&notifications={notify}",
            server_url = self.server_url,
            appid = self.appid,
            cluster = self.cluster_name,
            notify = urlencoding::encode(&notify)
        )
    }
}

impl<T> Config<T>
where
    T: serde::de::DeserializeOwned,
{
    pub async fn from_apollo(client: &ApolloClient) -> Result<Self, Error> {
        let (raw_str, _) = client.fetch().await?;
        let config: Config<T> = Raw {
            raw_str,
            typ: client.namespace_type,
        }
        .try_into()?;
        Ok(config)
    }
}

impl ApolloWatcher {
    fn watch_interval(
        &mut self,
        period: Duration,
        client: ApolloClient,
        topic: broadcast::Sender<Raw>,
    ) -> Result<(), Error> {
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                let ret = if period > CACHE_FETCH_THRESHOLD {
                    client.fetch().await.map(|v| v.0)
                } else {
                    client.cached_fetch().await
                };
                if let Ok(raw_str) = ret {
                    if topic
                        .send(Raw {
                            raw_str,
                            typ: client.namespace_type,
                        })
                        .is_err()
                    {
                        error!("[dynamic config] Unable to send fetched configuration updates from Apollo, DynamicConfigWatcher may work incorrectly");
                        break;
                    }
                    trace!("[dynamic config] Successfully fetched configuration from Apollo")
                } else {
                    error!("[dynamic config] Unable to fetch configuration updates from Apollo")
                }
            }
        });
        self.handle = Some(handle);
        Ok(())
    }

    fn watch_long_polling(
        &mut self,
        client: ApolloClient,
        topic: broadcast::Sender<Raw>,
    ) -> Result<(), Error> {
        let handle = tokio::spawn(async move {
            let mut notify_id: isize = -1;
            loop {
                if let Ok(new_id) = client.notification_fetch(notify_id).await {
                    if new_id == notify_id {
                        trace!("[dynamic config] notification id is already the latest, continue to fetch a new notification");
                        continue;
                    }
                    trace!("[dynamic config] notification id updated from {} to {}, start to fetch configuration", notify_id, new_id);
                    notify_id = new_id;
                    if let Ok((raw_str, _)) = client.fetch().await {
                        if topic
                            .send(Raw {
                                raw_str,
                                typ: client.namespace_type,
                            })
                            .is_err()
                        {
                            error!("[dynamic config] Unable to send fetched configuration updates from Apollo, DynamicConfigWatcher may work incorrectly");
                            break;
                        }
                        trace!("[dynamic config] Successfully fetched configuration from Apollo")
                    } else {
                        error!("[dynamic config] Unable to fetch configuration updates from Apollo")
                    }
                } else {
                    error!("[dynamic config] Unable to fetch notifications from Apollo")
                }
            }
        });
        self.handle = Some(handle);
        Ok(())
    }
}

impl InnerWatcher for ApolloWatcher {
    fn watch(&mut self) -> Result<(), Error> {
        self.stop()?;
        let client = self.client.clone();
        let topic = self.tx.clone();
        match self.mode {
            WatchMode::RealTime => self.watch_long_polling(client, topic),
            WatchMode::Interval(period) => self.watch_interval(period, client, topic),
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        if let Some(handler) = self.handle.take() {
            handler.abort()
        }
        Ok(())
    }
}

impl<T> DynamicConfig<T>
where
    T: serde::de::DeserializeOwned + Clone + 'static,
{
    pub async fn watch_apollo(
        client: ApolloClient,
        mode: WatchMode,
    ) -> (Self, DynamicConfigWatcher<T, ApolloWatcher>) {
        let (topic, _) = broadcast::channel::<Raw>(DEFAULT_BUFFER_SIZE);
        let config = Config::<T>::from_apollo(&client).await.unwrap();
        let config = Arc::new(Mutex::new(config));
        let inner_watcher = ApolloWatcher {
            client,
            tx: topic.clone(),
            mode,
            handle: None,
        };
        let watcher = DynamicConfigWatcher {
            inner: Arc::clone(&config),
            inner_watcher,
            handle: None,
            topic,
            verbose: false,
        };
        (Self(config), watcher)
    }

    // TODO implement webhook dynamic configuration
    // pub fn webhook_apollo() {
    //     unimplemented!()
    // }
}

// #[cfg(test)]
// mod test {
//     use super::*;
//
//     #[tokio::test]
//     async fn apollo_test() {
//         let client = ApolloClient::new("http://localhost:8080")
//             .appid("114514")
//             .namespace("test", ConfigType::TOML);
//         let (config, mut watcher) =
//             DynamicConfig::<Entry>::watch_apollo(client, WatchMode::RealTime).await;
//         watcher.verbose();
//         watcher.watch().unwrap();
//         println!("{:?}", config);
//         tokio::time::sleep(Duration::from_secs(10)).await;
//         println!("{:?}", config);
//     }
// }
