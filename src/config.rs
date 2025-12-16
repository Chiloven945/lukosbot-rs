use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::{fs, path::Path};

const CONFIG_DIR: &str = "config";
const CONFIG_FILE: &str = "config/application.yml";

const TEMPLATE_YAML: &str = r#"
lukos:
  prefix: "/"
  language: "zh-cn"
  telegram:
    enabled: false
    botToken: ""
    botUsername: ""
  discord:
    enabled: false
    token: ""
  onebot:
    enabled: false
    wsUrl: "ws://127.0.0.1:6700"
    accessToken: ""
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProperties {
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default)]
    pub telegram: Telegram,
    #[serde(default)]
    pub discord: Discord,
    #[serde(default)]
    pub onebot: Onebot,
}

fn default_prefix() -> String {
    "/".to_string()
}
fn default_lang() -> String {
    "zh-cn".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Telegram {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "botToken", default)]
    pub bot_token: String,
    #[serde(rename = "botUsername", default)]
    pub bot_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Discord {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Onebot {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "wsUrl", default = "default_ws")]
    pub ws_url: String,
    #[serde(rename = "accessToken", default)]
    pub access_token: String,
}

fn default_ws() -> String {
    "ws://127.0.0.1:6700".to_string()
}

pub struct ConfigLifecycle {
    running: bool,
}

impl ConfigLifecycle {
    pub fn new() -> Self {
        Self { running: false }
    }

    pub fn start(&mut self) -> Result<()> {
        if !Path::new(CONFIG_FILE).exists() {
            fs::create_dir_all(CONFIG_DIR).context("create config dir")?;
            fs::write(CONFIG_FILE, TEMPLATE_YAML).context("write default application.yml")?;
        }

        let defaults_v: Value =
            serde_yaml::from_str(TEMPLATE_YAML).context("parse template yaml")?;
        let user_txt = fs::read_to_string(CONFIG_FILE).context("read application.yml")?;
        let user_v: Value =
            serde_yaml::from_str(&user_txt).unwrap_or(Value::Mapping(Mapping::new()));

        let merged = deep_merge_ordered(&defaults_v, &user_v);

        let canonical = serde_yaml::to_string(&merged).context("dump canonical yaml")?;
        if normalize_lf(&user_txt) != normalize_lf(&canonical) {
            fs::write(CONFIG_FILE, canonical).context("rewrite application.yml")?;
        }

        self.running = true;
        Ok(())
    }

    pub fn load_props(&self) -> Result<AppProperties> {
        let txt = fs::read_to_string(CONFIG_FILE).context("read application.yml")?;
        let root: Value = serde_yaml::from_str(&txt).context("parse application.yml")?;
        let lukos = root
            .get("lukos")
            .cloned()
            .unwrap_or(Value::Mapping(Mapping::new()));
        let props: AppProperties = serde_yaml::from_value(lukos).context("decode lukos.*")?;
        Ok(props)
    }
}

fn normalize_lf(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

fn deep_merge_ordered(defaults: &Value, user: &Value) -> Value {
    match (defaults, user) {
        (Value::Mapping(dm), Value::Mapping(um)) => {
            let mut out = Mapping::new();

            for (k, dv) in dm.iter() {
                if let Some(uv) = um.get(k) {
                    let merged = deep_merge_ordered(dv, uv);
                    out.insert(k.clone(), merged);
                } else {
                    out.insert(k.clone(), dv.clone());
                }
            }

            for (k, uv) in um.iter() {
                if !out.contains_key(k) {
                    out.insert(k.clone(), uv.clone());
                }
            }

            Value::Mapping(out)
        }
        (_, uv) => uv.clone(),
    }
}
