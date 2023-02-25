use crate::{
    Config, ConfigType, DynamicConfig, DynamicConfigWatcher, Error, InnerWatcher, Raw,
    DEFAULT_BUFFER_SIZE,
};
use parking_lot::Mutex;
use reqwest::{Client, IntoUrl, Url};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{trace, warn};

pub struct Builder {
    server_url: Url,
    // Require
    data_id: String,
    // Require
    group: String,
    // Require
    config_type: ConfigType,
    // Require
    credential: Option<(String, String)>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            server_url: Url::parse("http://localhost:8848").expect("Url::parse"),
            data_id: "test".into(),
            group: "DEFAULT_GROUP".into(),
            config_type: ConfigType::YAML,
            credential: None,
        }
    }
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn server_url(mut self, url: impl IntoUrl) -> Self {
        self.server_url = url.into_url().expect("Into url failed");
        self
    }

    pub fn data_id(mut self, data_id: impl ToString) -> Self {
        self.data_id = data_id.to_string();
        self
    }

    pub fn config_type(mut self, config_type: ConfigType) -> Self {
        self.config_type = config_type;
        self
    }

    pub fn credential(mut self, username: impl ToString, password: impl ToString) -> Self {
        self.credential = Some((username.to_string(), password.to_string()));
        self
    }

    pub fn group(mut self, group: impl ToString) -> Self {
        self.group = group.to_string();
        self
    }

    pub fn finish(self) -> NacosClient {
        let url = Url::parse(&format!(
            "{server_url}/nacos/v1/cs/configs?dataId={data_id}&group={group}",
            server_url = self.server_url.as_str().trim_end_matches('/'),
            data_id = self.data_id,
            group = self.group,
        ))
        .expect("Url::parse");
        let endpoint = match self.credential {
            None => Endpoint::Normal(url),
            Some(credential) => Endpoint::Auth {
                url: Arc::new(Mutex::new(url)),
                login_url: Url::parse(&format!(
                    "{}/nacos/v1/auth/login",
                    self.server_url.as_str().trim_end_matches('/')
                ))
                .expect("Url::parse"),
                credential,
            },
        };
        NacosClient {
            http_client: Client::new(),
            endpoint,
            typ: self.config_type,
        }
    }
}

#[derive(Clone)]
enum Endpoint {
    Normal(Url),
    Auth {
        url: Arc<Mutex<Url>>,
        login_url: Url,
        credential: (String, String),
    },
}

impl Endpoint {
    #[inline]
    fn to_url(&self) -> Url {
        match self {
            Endpoint::Normal(v) => v.clone(),
            Endpoint::Auth { url, .. } => {
                let guard = url.lock();
                guard.clone()
            }
        }
    }

    #[inline]
    async fn update_access_token(&self, client: &NacosClient) -> Result<Url, Error> {
        if let Endpoint::Auth {
            url,
            login_url,
            credential,
        } = self
        {
            let new_access = client
                .http_client
                .post(login_url.clone())
                .header("content-type", "application/x-www-form-urlencoded")
                .body(format!(
                    "username={}&password={}",
                    credential.0, credential.1
                ))
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await?
                .get("accessToken")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| "".to_string());
            let mut guard = url.lock();
            let mut queries: Vec<_> = guard
                .query_pairs()
                .into_iter()
                .filter(|(k, _)| k != "accessToken")
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            queries.push(("accessToken".to_string(), new_access));
            guard.query_pairs_mut().clear();
            let new_url = Url::parse_with_params(guard.as_ref(), queries)?;
            *guard = new_url;
            return Ok(guard.clone());
        }
        Ok(self.to_url())
    }

    #[inline]
    async fn fetch(&self, client: &NacosClient) -> Result<String, Error> {
        let mut resp = client.http_client.get(self.to_url()).send().await?;
        if resp.status().is_client_error() {
            // token expired
            let new_url = self.update_access_token(client).await?;
            resp = client.http_client.get(new_url).send().await?;
        }
        resp.error_for_status_ref()?;
        let raw = resp.text().await?;
        Ok(raw)
    }
}

#[derive(Clone)]
pub struct NacosClient {
    http_client: Client,
    endpoint: Endpoint,
    typ: ConfigType,
}

#[cfg(feature = "dynamic")]
pub struct NacosWatcher {
    client: NacosClient,
    tx: broadcast::Sender<Raw>,
    handle: Option<JoinHandle<()>>,
    period: Duration,
}

impl NacosClient {
    pub async fn fetch(&self) -> Result<String, Error> {
        self.endpoint.fetch(self).await
    }
}

impl<T> Config<T>
where
    T: serde::de::DeserializeOwned,
{
    pub async fn from_nacos(client: &NacosClient) -> Result<Self, Error> {
        let raw_str = client.fetch().await?;
        let config: Config<T> = Raw {
            raw_str,
            typ: client.typ,
        }
        .try_into()?;
        Ok(config)
    }
}

#[cfg(feature = "dynamic")]
impl InnerWatcher for NacosWatcher {
    fn watch(&mut self) -> Result<(), Error> {
        self.stop()?;
        let client = self.client.clone();
        let topic = self.tx.clone();
        let period = self.period;
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                let ret = client.fetch().await;
                if let Ok(raw_str) = ret {
                    if topic
                        .send(Raw {
                            raw_str,
                            typ: client.typ,
                        })
                        .is_err()
                    {
                        warn!("[KOSEI] unable to send fetched configuration updates from Nacos, DynamicConfigWatcher may work incorrectly");
                        break;
                    }
                    trace!("[KOSEI] successfully fetched configuration from Nacos")
                } else {
                    warn!("[KOSEI] unable to fetch configuration updates from Nacos")
                }
            }
        });
        self.handle = Some(handle);
        Ok(())
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
    pub async fn watch_nacos(
        client: NacosClient,
        period: Duration,
    ) -> (Self, DynamicConfigWatcher<T, NacosWatcher>) {
        let (topic, _) = broadcast::channel::<Raw>(DEFAULT_BUFFER_SIZE);
        let config = Config::<T>::from_nacos(&client).await.unwrap();
        let config = Arc::new(Mutex::new(config));
        let inner_watcher = NacosWatcher {
            client,
            tx: topic.clone(),
            handle: None,
            period,
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
