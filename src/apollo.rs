use super::*;
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{error, trace};

const CACHE_FETCH_THRESHOLD: Duration = Duration::from_secs(30 * 60);
// 30 minutes
const APOLLO_HTTP_HEADER_AUTHORIZATION: &str = "Authorization";
const APOLLO_HTTP_HEADER_TIMESTAMP: &str = "Timestamp";
const APOLLO_SIG_DELIMITER: &str = "\n";

#[derive(Clone)]
pub struct ApolloClient {
    server_url: String,
    appid: String,
    cluster_name: String,
    namespace_name: String,
    namespace_type: ConfigType,
    release_key: String,
    local_ip: String,
    secret: Option<String>,
}

// releaseKey
#[derive(Debug, Default, Clone)]
pub struct Metadata(String);

#[cfg(feature = "dynamic")]
pub enum WatchMode {
    RealTime,
    // RealTime configuration updating (about 1s, based on Http long polling)
    Interval(Duration), // Each interval is updated, preferably greater than 30s, several hours or days are recommended
}

#[cfg(feature = "dynamic")]
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
            secret: None,
        }
    }
}

impl ApolloClient {
    // a http uri, such as 'http://localhost:8080'
    pub fn new(uri: &str) -> Self {
        Self {
            server_url: uri.trim_end_matches('/').to_string(),
            ..Default::default()
        }
    }

    // Required
    pub fn appid(mut self, id: &str) -> Self {
        self.appid = id.to_string();
        self
    }

    // Required, default set to `default`
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

    pub fn secret(mut self, access_secret: &str) -> Self {
        self.secret = Some(access_secret.to_string());
        self
    }

    pub fn some_secret(mut self, access_secret: Option<&str>) -> Self {
        self.secret = access_secret.map(ToString::to_string);
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

    // Not support `.properties`
    // NOTICE: make sure namespace had been published and set to a support config type
    // (responses should contains `configurations.content`)
    pub async fn fetch(&self) -> Result<(String, Metadata), Error> {
        let url = self.nocache_url();
        let resp = self.request_builder(&url).build()?.get(url).send().await?;

        resp.error_for_status_ref()?;

        let resp = resp.json::<serde_json::Value>().await?;

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
        let url = self.cached_url();
        let resp = self.request_builder(&url).build()?.get(url).send().await?;

        resp.error_for_status_ref()?;

        let config = resp
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
        let url = self.notify_url(notify_id);
        let resp = self
            .request_builder(&url)
            .timeout(Duration::from_secs(70))
            .build()?
            .get(url)
            .send()
            .await?;

        resp.error_for_status_ref()?;

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

    fn request_builder(&self, url: &str) -> reqwest::ClientBuilder {
        let client_builder = reqwest::Client::builder();
        if let Some(secret) = &self.secret {
            let path_with_query = url.trim_start_matches(&self.server_url);

            let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret.as_bytes())
                .expect("HMAC can take key of any size");

            let timestamp_millis = {
                let now = std::time::SystemTime::now();
                now.duration_since(std::time::UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_millis()
            };

            mac.update(
                format!(
                    "{}{}{}",
                    timestamp_millis, APOLLO_SIG_DELIMITER, path_with_query
                )
                    .as_bytes(),
            );

            let sig_arr = mac.finalize().into_bytes();
            let sig = base64::encode(sig_arr);

            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                APOLLO_HTTP_HEADER_AUTHORIZATION,
                format!("Apollo {}:{}", self.appid, sig).parse().unwrap(),
            );
            headers.insert(
                APOLLO_HTTP_HEADER_TIMESTAMP,
                timestamp_millis.to_string().parse().unwrap(),
            );

            return client_builder.default_headers(headers);
        }
        client_builder
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

#[cfg(feature = "dynamic")]
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

#[cfg(feature = "dynamic")]
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

#[cfg(feature = "dynamic")]
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
}
