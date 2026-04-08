use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub events: Vec<EventConfig>,
    pub deployment: DeploymentConfig,
    pub browser: BrowserConfig,
    pub captcha: CaptchaConfig,
    pub notifications: NotificationConfig,
    pub proxy: ProxyConfig,
    pub trace: TraceConfig,
    pub sessions: Vec<SessionConfig>,
    pub ticketmaster_api_key: String,
    #[serde(skip)]
    pub base_dir: PathBuf,
}

impl AppConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let config_path = path.as_ref();
        let env_path = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".env");
        Self::load_from_paths(config_path, env_path)
    }

    pub fn load_from_paths(
        config_path: impl AsRef<Path>,
        env_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let config_path = config_path.as_ref().canonicalize().with_context(|| {
            format!(
                "failed to resolve config path: {}",
                config_path.as_ref().display()
            )
        })?;
        let env_overrides = load_dotenv_map(env_path.as_ref())?;
        Self::load_with_env(&config_path, |key| {
            env_overrides
                .get(key)
                .cloned()
                .or_else(|| std::env::var(key).ok())
        })
    }

    fn load_with_env<F>(config_path: &Path, env_lookup: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let base_dir = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let raw = fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path.display()))?;
        let root_value: Value = serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse YAML config: {}", config_path.display()))?;
        let root = root_value.as_mapping().cloned().unwrap_or_default();

        let deployment_profile = normalize_deployment_profile(
            env_lookup("DEPLOYMENT_PROFILE")
                .or_else(|| section_string(&root, "deployment", "profile"))
                .unwrap_or_default()
                .as_str(),
        );
        let deployment = DeploymentConfig {
            profile: deployment_profile.clone(),
        };
        let preset_root = deployment_preset(&deployment_profile);

        let events: Vec<EventConfig> = serde_yaml::from_value(
            root_section(&root, "events").unwrap_or(Value::Sequence(vec![])),
        )
        .context("failed to parse events config")?;

        let mut browser_value = deep_merge(
            root_section(&preset_root, "browser"),
            root_section(&root, "browser"),
        );
        if let Some(node_id) = env_lookup("NODE_ID").filter(|value| !value.trim().is_empty()) {
            set_yaml_string(
                &mut browser_value,
                "user_data_dir",
                format!("./chrome_profile_node_{}", node_id.trim()),
            );
        }
        if let Some(engine) = env_lookup("BROWSER_ENGINE").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string(&mut browser_value, "engine", engine);
        }
        if let Some(headless) = env_lookup("BROWSER_HEADLESS") {
            set_yaml_bool(&mut browser_value, "headless", parse_env_bool(&headless));
        }
        if let Some(path) =
            env_lookup("BROWSER_EXECUTABLE_PATH").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string(&mut browser_value, "executable_path", path);
        }
        if let Some(api_mode) =
            env_lookup("BROWSER_API_MODE").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string(&mut browser_value, "api_mode", api_mode);
        }
        let mut browser: BrowserConfig =
            serde_yaml::from_value(browser_value).context("failed to parse browser config")?;
        browser.user_data_dir = absolutize_relative_path(&base_dir, &browser.user_data_dir);

        let mut captcha_value = deep_merge(
            root_section(&preset_root, "captcha"),
            root_section(&root, "captcha"),
        );
        if let Some(collect_dir) =
            env_lookup("CAPTCHA_COLLECT_DIR").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string(&mut captcha_value, "collect_dir", collect_dir);
        } else if env_lookup("CAPTCHA_COLLECT_ENABLED")
            .map(|value| parse_env_bool(&value))
            .unwrap_or(false)
            && yaml_string(&captcha_value, "collect_dir")
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            set_yaml_string(
                &mut captcha_value,
                "collect_dir",
                "./captcha_samples".to_string(),
            );
        }
        let mut captcha: CaptchaConfig =
            serde_yaml::from_value(captcha_value).context("failed to parse captcha config")?;
        captcha.custom_model_path = absolutize_relative_path(&base_dir, &captcha.custom_model_path);
        captcha.custom_charset_path =
            absolutize_relative_path(&base_dir, &captcha.custom_charset_path);
        captcha.collect_dir = absolutize_relative_path(&base_dir, &captcha.collect_dir);

        let notifications_value = deep_merge(
            root_section(&preset_root, "notifications"),
            root_section(&root, "notifications"),
        );
        let telegram_default: TelegramConfig =
            serde_yaml::from_value(section_from_value(&notifications_value, "telegram"))
                .context("failed to parse telegram config")?;
        let discord_default: DiscordConfig =
            serde_yaml::from_value(section_from_value(&notifications_value, "discord"))
                .context("failed to parse discord config")?;
        let notifications = NotificationConfig {
            telegram: TelegramConfig {
                enabled: telegram_default.enabled,
                bot_token: env_lookup("TELEGRAM_BOT_TOKEN").unwrap_or_default(),
                chat_id: if !telegram_default.chat_id.trim().is_empty() {
                    telegram_default.chat_id
                } else {
                    env_lookup("TELEGRAM_CHAT_ID").unwrap_or_default()
                },
            },
            discord: DiscordConfig {
                enabled: discord_default.enabled,
                webhook_url: env_lookup("DISCORD_WEBHOOK_URL").unwrap_or_default(),
            },
        };

        let mut proxy_value = deep_merge(
            root_section(&preset_root, "proxy"),
            root_section(&root, "proxy"),
        );
        if let Some(enabled) = env_lookup("PROXY_ENABLED") {
            set_yaml_bool(&mut proxy_value, "enabled", parse_env_bool(&enabled));
        }
        if let Some(rotate) = env_lookup("PROXY_ROTATE") {
            set_yaml_bool(&mut proxy_value, "rotate", parse_env_bool(&rotate));
        }
        if let Some(servers) = env_lookup("PROXY_SERVERS").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string_list(&mut proxy_value, "servers", parse_env_list(&servers));
        }
        let proxy: ProxyConfig =
            serde_yaml::from_value(proxy_value).context("failed to parse proxy config")?;

        let mut trace_value = deep_merge(
            root_section(&preset_root, "trace"),
            root_section(&root, "trace"),
        );
        if let Some(enabled) = env_lookup("TIXCRAFT_TRACE_HEADERS") {
            set_yaml_bool(&mut trace_value, "enabled", parse_env_bool(&enabled));
        }
        if let Some(log_path) =
            env_lookup("TIXCRAFT_TRACE_LOG_PATH").filter(|value| !value.trim().is_empty())
        {
            set_yaml_string(&mut trace_value, "log_path", log_path);
        }
        let mut trace: TraceConfig =
            serde_yaml::from_value(trace_value).context("failed to parse trace config")?;
        trace.log_path = absolutize_relative_path(&base_dir, &trace.log_path);

        let sessions_value = root_section(&root, "sessions").unwrap_or(Value::Sequence(vec![]));
        let mut sessions: Vec<SessionConfig> =
            serde_yaml::from_value(sessions_value).context("failed to parse sessions config")?;
        if sessions.is_empty() {
            sessions.push(SessionConfig {
                name: "default".to_string(),
                user_data_dir: browser.user_data_dir.clone(),
                ..SessionConfig::default()
            });
        }
        for session in &mut sessions {
            session.user_data_dir = absolutize_relative_path(&base_dir, &session.user_data_dir);
            session.cookie_file = absolutize_relative_path(&base_dir, &session.cookie_file);
        }

        Ok(Self {
            events,
            deployment,
            browser,
            captcha,
            notifications,
            proxy,
            trace,
            sessions,
            ticketmaster_api_key: env_lookup("TICKETMASTER_API_KEY").unwrap_or_default(),
            base_dir,
        })
    }

    pub fn select_event(&self, keyword: Option<&str>) -> Option<EventConfig> {
        match keyword {
            Some(keyword) if !keyword.trim().is_empty() => {
                let needle = keyword.trim().to_lowercase();
                self.events
                    .iter()
                    .find(|event| {
                        event.name.to_lowercase().contains(&needle)
                            || event.url.to_lowercase().contains(&needle)
                    })
                    .cloned()
            }
            _ => self.events.first().cloned(),
        }
    }

    pub fn select_session(&self, session_name: Option<&str>) -> Option<SessionConfig> {
        match session_name {
            Some(session_name) if !session_name.trim().is_empty() => self
                .sessions
                .iter()
                .find(|session| session.name == session_name)
                .cloned(),
            _ => self.sessions.first().cloned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EventConfig {
    pub name: String,
    pub platform: String,
    pub url: String,
    pub ticket_count: u32,
    pub date_keyword: String,
    pub area_keyword: String,
    pub sale_time: String,
    pub presale_code: String,
}

impl Default for EventConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            platform: String::new(),
            url: String::new(),
            ticket_count: 2,
            date_keyword: String::new(),
            area_keyword: String::new(),
            sale_time: String::new(),
            presale_code: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub engine: String,
    pub headless: bool,
    pub user_data_dir: String,
    pub pre_warm: bool,
    pub lang: String,
    pub executable_path: String,
    pub api_mode: String,
    pub turbo_mode: bool,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            engine: "nodriver".to_string(),
            headless: false,
            user_data_dir: "./chrome_profile".to_string(),
            pre_warm: true,
            lang: "zh-TW".to_string(),
            executable_path: "/usr/bin/chromium".to_string(),
            api_mode: "off".to_string(),
            turbo_mode: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CaptchaConfig {
    pub engine: String,
    pub beta_model: bool,
    pub char_ranges: u32,
    pub confidence_threshold: f32,
    pub max_attempts: u32,
    pub preprocess: bool,
    pub custom_model_path: String,
    pub custom_charset_path: String,
    pub collect_dir: String,
}

impl Default for CaptchaConfig {
    fn default() -> Self {
        Self {
            engine: "ddddocr".to_string(),
            beta_model: true,
            char_ranges: 1,
            confidence_threshold: 0.6,
            max_attempts: 5,
            preprocess: true,
            custom_model_path: String::new(),
            custom_charset_path: String::new(),
            collect_dir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct NotificationConfig {
    pub telegram: TelegramConfig,
    pub discord: DiscordConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DiscordConfig {
    pub enabled: bool,
    pub webhook_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub enabled: bool,
    pub rotate: bool,
    pub servers: Vec<String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rotate: true,
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TraceConfig {
    pub enabled: bool,
    pub log_path: String,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_path: "./logs/tixcraft_trace.jsonl".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DeploymentConfig {
    pub profile: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub name: String,
    pub user_data_dir: String,
    pub proxy_server: String,
    pub cookie_file: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            user_data_dir: "./chrome_profile".to_string(),
            proxy_server: String::new(),
            cookie_file: String::new(),
        }
    }
}

fn load_dotenv_map(path: &Path) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    if !path.exists() {
        return Ok(vars);
    }

    let iter = dotenvy::from_path_iter(path)
        .with_context(|| format!("failed to read env file: {}", path.display()))?;
    for item in iter {
        let (key, value) =
            item.with_context(|| format!("failed to parse env file: {}", path.display()))?;
        vars.insert(key, value);
    }
    Ok(vars)
}

fn normalize_deployment_profile(profile: &str) -> String {
    let key = profile.trim().to_lowercase().replace('-', "_");
    match key.as_str() {
        "local" | "local_desktop" => "local_desktop".to_string(),
        "gcp" | "cloud" | "gcp_taiwan" => "gcp_taiwan".to_string(),
        "aws" | "tokyo" | "aws_tokyo" => "aws_tokyo".to_string(),
        other => other.to_string(),
    }
}

fn parse_env_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_env_list(raw: &str) -> Vec<String> {
    raw.replace('\n', ",")
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn absolutize_relative_path(base_dir: &Path, raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        trimmed.to_string()
    } else {
        base_dir
            .join(path)
            .components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    }
}

fn deployment_preset(profile: &str) -> Mapping {
    let yaml = match profile {
        "local_desktop" => {
            r#"
browser:
  engine: nodriver
  headless: false
  user_data_dir: "./chrome_profile"
  pre_warm: true
  lang: "zh-TW"
  executable_path: ""
  api_mode: "full"
  turbo_mode: true
captcha:
  engine: ddddocr
  beta_model: true
  char_ranges: 0
  confidence_threshold: 0.6
  max_attempts: 5
  preprocess: false
  custom_model_path: "model/captcha_model.onnx"
  custom_charset_path: "model/charset.json"
  collect_dir: "./captcha_samples"
notifications:
  telegram:
    enabled: true
  discord:
    enabled: true
proxy:
  enabled: false
  rotate: true
  servers: []
trace:
  enabled: true
  log_path: "./logs/tixcraft_trace_local.jsonl"
"#
        }
        "gcp_taiwan" => {
            r#"
browser:
  engine: playwright
  headless: true
  user_data_dir: "./chrome_profile_node_1"
  pre_warm: true
  lang: "zh-TW"
  executable_path: "/usr/bin/chromium"
  api_mode: "full"
  turbo_mode: true
captcha:
  engine: ddddocr
  beta_model: true
  char_ranges: 0
  confidence_threshold: 0.6
  max_attempts: 5
  preprocess: false
  custom_model_path: "model/captcha_model.onnx"
  custom_charset_path: "model/charset.json"
  collect_dir: ""
notifications:
  telegram:
    enabled: true
  discord:
    enabled: true
proxy:
  enabled: false
  rotate: true
  servers: []
trace:
  enabled: true
  log_path: "./logs/tixcraft_trace_cloud.jsonl"
"#
        }
        "aws_tokyo" => {
            r#"
browser:
  engine: playwright
  headless: true
  user_data_dir: "./chrome_profile_node_1"
  pre_warm: true
  lang: "zh-TW"
  executable_path: "/usr/bin/chromium"
  api_mode: "full"
  turbo_mode: true
captcha:
  engine: ddddocr
  beta_model: true
  char_ranges: 0
  confidence_threshold: 0.6
  max_attempts: 5
  preprocess: false
  custom_model_path: "model/captcha_model.onnx"
  custom_charset_path: "model/charset.json"
  collect_dir: ""
notifications:
  telegram:
    enabled: false
  discord:
    enabled: false
proxy:
  enabled: false
  rotate: true
  servers: []
trace:
  enabled: true
  log_path: "./logs/tixcraft_trace_aws_tokyo.jsonl"
"#
        }
        _ => return Mapping::new(),
    };

    serde_yaml::from_str::<Mapping>(yaml).unwrap_or_default()
}

fn deep_merge(base: Option<Value>, override_value: Option<Value>) -> Value {
    match (base, override_value) {
        (Some(Value::Mapping(mut base_map)), Some(Value::Mapping(override_map))) => {
            for (key, value) in override_map {
                let existing = base_map.remove(&key);
                base_map.insert(key, deep_merge(existing, Some(value)));
            }
            Value::Mapping(base_map)
        }
        (_, Some(value)) => value,
        (Some(value), None) => value,
        (None, None) => Value::Mapping(Mapping::new()),
    }
}

fn root_section(root: &Mapping, key: &str) -> Option<Value> {
    root.get(Value::String(key.to_string())).cloned()
}

fn section_from_value(value: &Value, key: &str) -> Value {
    value
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_string())).cloned())
        .unwrap_or_else(|| Value::Mapping(Mapping::new()))
}

fn section_string(root: &Mapping, section: &str, field: &str) -> Option<String> {
    root_section(root, section)
        .and_then(|value| value.as_mapping().cloned())
        .and_then(|mapping| mapping.get(Value::String(field.to_string())).cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn yaml_string(value: &Value, key: &str) -> Option<String> {
    value
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_string())))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn set_yaml_string(value: &mut Value, key: &str, new_value: String) {
    ensure_mapping(value).insert(Value::String(key.to_string()), Value::String(new_value));
}

fn set_yaml_bool(value: &mut Value, key: &str, new_value: bool) {
    ensure_mapping(value).insert(Value::String(key.to_string()), Value::Bool(new_value));
}

fn set_yaml_string_list(value: &mut Value, key: &str, new_value: Vec<String>) {
    let sequence = new_value.into_iter().map(Value::String).collect();
    ensure_mapping(value).insert(Value::String(key.to_string()), Value::Sequence(sequence));
}

fn ensure_mapping(value: &mut Value) -> &mut Mapping {
    if !matches!(value, Value::Mapping(_)) {
        *value = Value::Mapping(Mapping::new());
    }
    match value {
        Value::Mapping(mapping) => mapping,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    struct TempFiles {
        config_path: PathBuf,
        env_path: PathBuf,
    }

    impl TempFiles {
        fn new(config_body: &str, env_body: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ticket-bot2-config-{}", Uuid::new_v4()));
            fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.yaml");
            let env_path = dir.join(".env");
            fs::write(&config_path, config_body).unwrap();
            fs::write(&env_path, env_body).unwrap();
            Self {
                config_path,
                env_path,
            }
        }
    }

    impl Drop for TempFiles {
        fn drop(&mut self) {
            let _ = remove_file_if_exists(&self.config_path);
            let _ = remove_file_if_exists(&self.env_path);
            if let Some(parent) = self.config_path.parent() {
                let _ = fs::remove_dir(parent);
            }
        }
    }

    fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
        if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        }
    }

    #[test]
    fn load_config_applies_presets_and_env_overrides() {
        let files = TempFiles::new(
            r#"
deployment:
  profile: local
events:
  - name: IVE
    platform: tixcraft
    url: https://tixcraft.com/activity/game/26_ive
browser:
  api_mode: checkout
notifications:
  telegram:
    chat_id: yaml-chat
proxy:
  enabled: false
"#,
            r#"
BROWSER_ENGINE=playwright
BROWSER_HEADLESS=true
TELEGRAM_BOT_TOKEN=test-token
DISCORD_WEBHOOK_URL=https://discord.test/webhook
PROXY_ENABLED=true
PROXY_SERVERS=http://a:80,http://b:80
TIXCRAFT_TRACE_HEADERS=true
TICKETMASTER_API_KEY=test-ticketmaster
"#,
        );

        let config = AppConfig::load_from_paths(&files.config_path, &files.env_path).unwrap();
        let base_dir = files.config_path.parent().unwrap().canonicalize().unwrap();

        assert_eq!(config.deployment.profile, "local_desktop");
        assert_eq!(config.browser.engine, "playwright");
        assert!(config.browser.headless);
        assert_eq!(config.browser.api_mode, "checkout");
        assert_eq!(
            config.captcha.custom_model_path,
            base_dir.join("model/captcha_model.onnx").to_string_lossy()
        );
        assert!(config.notifications.telegram.enabled);
        assert_eq!(config.notifications.telegram.bot_token, "test-token");
        assert_eq!(config.notifications.telegram.chat_id, "yaml-chat");
        assert!(config.notifications.discord.enabled);
        assert_eq!(
            config.notifications.discord.webhook_url,
            "https://discord.test/webhook"
        );
        assert!(config.proxy.enabled);
        assert_eq!(config.proxy.servers, vec!["http://a:80", "http://b:80"]);
        assert!(config.trace.enabled);
        assert_eq!(config.ticketmaster_api_key, "test-ticketmaster");
        assert_eq!(config.sessions.len(), 1);
        assert_eq!(
            config.sessions[0].user_data_dir,
            config.browser.user_data_dir
        );
        assert_eq!(config.base_dir, base_dir);
    }

    #[test]
    fn load_config_respects_node_id_and_collect_dir_fallback() {
        let files = TempFiles::new(
            r#"
deployment:
  profile: aws
events:
  - name: EXO
    platform: tixcraft
    url: https://tixcraft.com/activity/game/26_exo
"#,
            r#"
NODE_ID=7
CAPTCHA_COLLECT_ENABLED=true
"#,
        );

        let config = AppConfig::load_from_paths(&files.config_path, &files.env_path).unwrap();
        let base_dir = files.config_path.parent().unwrap().canonicalize().unwrap();

        assert_eq!(config.deployment.profile, "aws_tokyo");
        assert_eq!(
            config.browser.user_data_dir,
            base_dir.join("chrome_profile_node_7").to_string_lossy()
        );
        assert_eq!(config.sessions.len(), 1);
        assert_eq!(
            config.sessions[0].user_data_dir,
            base_dir.join("chrome_profile_node_7").to_string_lossy()
        );
        assert_eq!(
            config.captcha.collect_dir,
            base_dir.join("captcha_samples").to_string_lossy()
        );
        assert!(!config.notifications.telegram.enabled);
    }
}
