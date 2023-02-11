use crate::{
    Config, ConfigType, DynamicConfig, DynamicConfigWatcher, Error, InnerWatcher, Raw,
    DEFAULT_BUFFER_SIZE,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{trace, warn};

#[derive(Clone)]
pub struct NacosClient {
    server_url: String,
    data_id: Option<String>,
    group: Option<String>,
    typ: Option<ConfigType>,
    credential: Option<(String, String)>,
    access_token: Option<String>,
}

#[cfg(feature = "dynamic")]
pub struct NacosWatcher {
    client: NacosClient,
    tx: broadcast::Sender<Raw>,
    handle: Option<JoinHandle<()>>,
    period: Duration,
}

impl NacosClient {
    pub fn new(uri: &str) -> Self {
        Self {
            server_url: uri.trim_end_matches('/').to_string(),
            data_id: None,
            group: Some("DEFAULT_GROUP".to_string()),
            typ: None,
            credential: None,
            access_token: None,
        }
    }

    // Required
    pub fn data_id(mut self, data_id: &str) -> Self {
        self.data_id = Some(data_id.to_string());
        self
    }

    // Required
    pub fn config_type(mut self, typ: ConfigType) -> Self {
        self.typ = Some(typ);
        self
    }

    // Optional
    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }

    pub fn credential(mut self, username: &str, password: &str) -> Self {
        self.credential = Some((username.to_string(), password.to_string()));
        self
    }

    pub async fn fetch(&mut self) -> Result<String, Error> {
        let mut resp = reqwest::get(self.url().await?).await?;
        if resp.status().is_client_error() {
            // token expired
            self.access_token.take();
            resp = reqwest::get(self.url().await?).await?;
        }
        resp.error_for_status_ref()?;
        let raw = resp.text().await?;
        Ok(raw)
    }

    async fn url(&mut self) -> Result<String, Error> {
        match self.credential {
            None => Ok(format!(
                "{server_url}/nacos/v1/cs/configs?dataId={data_id}&group={group}",
                server_url = self.server_url,
                data_id = self.data_id.as_deref().expect("Require data id defined"),
                group = self.group.as_deref().expect("Require group defined"),
            )),
            Some((ref username, ref password)) => {
                let access = match self.access_token {
                    Some(ref access) => access.to_string(),
                    None => {
                        let login_url = format!("{}/nacos/v1/auth/login", self.server_url);
                        let resp = reqwest::Client::builder()
                            .build()?
                            .post(login_url)
                            .header("content-type", "application/x-www-form-urlencoded")
                            .body(format!("username={}&password={}", username, password))
                            .send()
                            .await?;
                        resp.error_for_status_ref()?;
                        let new_access = resp
                            .json::<serde_json::Value>()
                            .await?
                            .get("accessToken")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "".to_string());
                        self.access_token = Some(new_access.clone());
                        new_access
                    }
                };
                Ok(format!(
                    "{server_url}/nacos/v1/cs/configs?dataId={data_id}&group={group}&accessToken={access}",
                    server_url = self.server_url,
                    data_id = self.data_id.as_deref().expect("Require data id defined"),
                    group = self.group.as_deref().expect("Require group defined"),
                    access = access
                ))
            }
        }
    }
}

impl<T> Config<T>
where
    T: serde::de::DeserializeOwned,
{
    pub async fn from_nacos(client: &mut NacosClient) -> Result<Self, Error> {
        let raw_str = client.fetch().await?;
        let config: Config<T> = Raw {
            raw_str,
            typ: client.typ.expect("Require config type defined"),
        }
        .try_into()?;
        Ok(config)
    }
}

#[cfg(feature = "dynamic")]
impl InnerWatcher for NacosWatcher {
    fn watch(&mut self) -> Result<(), Error> {
        self.stop()?;
        let mut client = self.client.clone();
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
                            typ: client.typ.expect("Require namespace defined"),
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
        mut client: NacosClient,
        period: Duration,
    ) -> (Self, DynamicConfigWatcher<T, NacosWatcher>) {
        let (topic, _) = broadcast::channel::<Raw>(DEFAULT_BUFFER_SIZE);
        let config = Config::<T>::from_nacos(&mut client).await.unwrap();
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
