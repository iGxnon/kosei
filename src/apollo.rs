use crate::{
    Config, ConfigType, DynamicConfig, DynamicConfigWatcher, Error, InnerWatcher, Raw,
    DEFAULT_BUFFER_SIZE,
};
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use reqwest::header::HeaderMap;
use reqwest::{Client, IntoUrl, Url};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{trace, warn};

// fetch notification will take about 1 min to respond, remain 30s for timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(90);
const CACHE_FETCH_THRESHOLD: Duration = Duration::from_secs(30 * 60);
const APOLLO_HTTP_HEADER_AUTHORIZATION: &str = "Authorization";
const APOLLO_HTTP_HEADER_TIMESTAMP: &str = "Timestamp";
const APOLLO_SIG_DELIMITER: &str = "\n";

#[derive(Clone)]
pub struct Builder {
    // Require
    server_url: Url,
    // Require
    app_id: String,
    // Require
    cluster_name: String,
    // Require
    namespace: String,
    // Require
    namespace_type: ConfigType,
    // Optional
    local_ip: Option<String>,
    // Optional
    secret: Option<String>,
}

type Endpoint<U> = (U, String);
// (url, path)
type NotifyEndpoint = (fn(&Builder, isize) -> Url, fn(&Builder, isize) -> String);

#[derive(Clone)]
struct Endpoints {
    nocache_endpoint: Endpoint<Url>,
    cached_endpoint: Endpoint<Url>,
    notify_endpoint: NotifyEndpoint,
    conf: Builder,
}

impl Endpoints {
    #[inline]
    async fn nocache_fetch(&self, client: &ApolloClient) -> Result<String, Error> {
        let config = client
            .http_client
            .get(self.nocache_endpoint.0.clone())
            .headers(self.conf.auth_headers(self.nocache_endpoint.1.as_str()))
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .get("configurations")
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "".to_string());
        Ok(config)
    }

    #[inline]
    async fn cache_fetch(&self, client: &ApolloClient) -> Result<String, Error> {
        let config = client
            .http_client
            .get(self.cached_endpoint.0.clone())
            .headers(self.conf.auth_headers(self.cached_endpoint.1.as_str()))
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?
            .get("content")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| "".to_string());
        Ok(config)
    }

    #[inline]
    async fn notification_fetch(
        &self,
        client: &ApolloClient,
        notify_id: isize,
    ) -> Result<isize, Error> {
        let resp = client
            .http_client
            .get(self.notify_endpoint.0(&self.conf, notify_id))
            .headers(
                self.conf
                    .auth_headers(self.notify_endpoint.1(&self.conf, notify_id).as_str()),
            )
            .send()
            .await?
            .error_for_status()?;
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
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            server_url: Url::parse("http://127.0.0.1:8080/").unwrap(),
            app_id: "default".into(),
            cluster_name: "default".into(),
            namespace: "config".into(),
            namespace_type: ConfigType::YAML,
            local_ip: None,
            secret: None,
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn server_url(mut self, url: impl IntoUrl) -> Self {
        self.server_url = url.into_url().expect("Not a valid url");
        self
    }

    pub fn app_id(mut self, app_id: impl ToString) -> Self {
        self.app_id = app_id.to_string();
        self
    }

    pub fn cluster(mut self, cluster: impl ToString) -> Self {
        self.cluster_name = cluster.to_string();
        self
    }

    pub fn namespace(mut self, ns: impl AsRef<str>, typ: ConfigType) -> Self {
        match typ {
            ConfigType::YAML => {
                self.namespace = format!("{}.yaml", ns.as_ref());
                self.namespace_type = ConfigType::YAML;
            }
            ConfigType::JSON => {
                self.namespace = format!("{}.json", ns.as_ref());
                self.namespace_type = ConfigType::JSON;
            }
            ConfigType::TOML => panic!("Apollo dose not support toml yet"),
        }
        self
    }

    // Optional
    pub fn secret(mut self, access_secret: &str) -> Self {
        self.secret = Some(access_secret.to_string());
        self
    }

    // Optional
    pub fn local_ip(mut self, ip: &str) -> Self {
        self.local_ip = Some(ip.to_string());
        self
    }

    pub fn finish(self) -> ApolloClient {
        let nocache_path = format!(
            "/configs/{app_id}/{cluster}/{namespace}?ip={ip}",
            app_id = self.app_id,
            cluster = self.cluster_name,
            namespace = self.namespace,
            ip = self.local_ip.as_deref().unwrap_or_default()
        );
        let cache_path = format!(
            "/configfiles/json/{app_id}/{cluster}/{namespace}?ip={ip}",
            app_id = self.app_id,
            cluster = self.cluster_name,
            namespace = self.namespace,
            ip = self.local_ip.as_deref().unwrap_or_default()
        );
        fn notify_path(conf: &Builder, notify_id: isize) -> String {
            let notify = format!(
                r#"[{{"namespaceName": "{}", "notificationId": {}}}]"#,
                conf.namespace, notify_id
            );
            format!(
                "/notifications/v2?appId={app_id}&cluster={cluster}&notifications={notify}",
                app_id = conf.app_id,
                cluster = conf.cluster_name,
                notify = urlencoding::encode(&notify)
            )
        }
        fn notify_url(conf: &Builder, notify_id: isize) -> Url {
            Url::parse(&format!(
                "{}{}",
                conf.server_url.as_str().trim_end_matches('/'),
                notify_path(conf, notify_id)
            ))
            .expect("Url::parse")
        }

        ApolloClient {
            http_client: Client::builder()
                .timeout(DEFAULT_TIMEOUT)
                .build()
                .expect("Cannot build http client"),
            endpoints: Endpoints {
                nocache_endpoint: (
                    Url::parse(&format!(
                        "{}{}",
                        self.server_url.as_str().trim_end_matches('/'),
                        nocache_path
                    ))
                    .expect("Url::parse"),
                    nocache_path,
                ),
                cached_endpoint: (
                    Url::parse(&format!(
                        "{}{}",
                        self.server_url.as_str().trim_end_matches('/'),
                        cache_path
                    ))
                    .expect("Url::parse"),
                    cache_path,
                ),
                notify_endpoint: (notify_url, notify_path),
                conf: self,
            },
        }
    }

    #[inline]
    fn auth_headers(&self, path_with_query: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(secret) = self.secret.as_deref() {
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

            headers.insert(
                APOLLO_HTTP_HEADER_AUTHORIZATION,
                format!("Apollo {}:{}", self.app_id, sig).parse().unwrap(),
            );
            headers.insert(
                APOLLO_HTTP_HEADER_TIMESTAMP,
                timestamp_millis.to_string().parse().unwrap(),
            );
        }
        headers
    }
}

#[derive(Clone)]
pub struct ApolloClient {
    http_client: Client,
    endpoints: Endpoints,
}

impl ApolloClient {
    pub async fn fetch(&self) -> Result<String, Error> {
        self.endpoints.nocache_fetch(self).await
    }

    pub async fn cached_fetch(&self) -> Result<String, Error> {
        self.endpoints.cache_fetch(self).await
    }

    pub async fn notification_fetch(&self, notify_id: isize) -> Result<isize, Error> {
        self.endpoints.notification_fetch(self, notify_id).await
    }
}

impl<T> Config<T>
where
    T: serde::de::DeserializeOwned,
{
    pub async fn from_apollo(client: &ApolloClient) -> Result<Self, Error> {
        let raw_str = client.fetch().await?;
        let config: Config<T> = Raw {
            raw_str,
            typ: client.endpoints.conf.namespace_type,
        }
        .try_into()?;
        Ok(config)
    }
}

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
                    client.fetch().await
                } else {
                    client.cached_fetch().await
                };
                if let Ok(raw_str) = ret {
                    if topic
                        .send(Raw {
                            raw_str,
                            typ: client.endpoints.conf.namespace_type,
                        })
                        .is_err()
                    {
                        warn!("[KOSEI] unable to send fetched configuration updates from Apollo, DynamicConfigWatcher may work incorrectly");
                        break;
                    }
                    trace!("[KOSEI] successfully fetched configuration from Apollo")
                } else {
                    warn!("[KOSEI] unable to fetch configuration updates from Apollo")
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
                        trace!("[KOSEI] notification id is already the latest, continue to fetch a new notification");
                        continue;
                    }
                    trace!("[KOSEI] notification id updated from {} to {}, start to fetch configuration", notify_id, new_id);
                    notify_id = new_id;
                    if let Ok(raw_str) = client.fetch().await {
                        if topic
                            .send(Raw {
                                raw_str,
                                typ: client.endpoints.conf.namespace_type,
                            })
                            .is_err()
                        {
                            warn!("[KOSEI] unable to send fetched configuration updates from Apollo, DynamicConfigWatcher may work incorrectly");
                            break;
                        }
                        trace!("[KOSEI] successfully fetched configuration from Apollo")
                    } else {
                        warn!("[KOSEI] unable to fetch configuration updates from Apollo")
                    }
                } else {
                    warn!("[KOSEI] unable to fetch notifications from Apollo")
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
        };
        (Self(config), watcher)
    }
}
