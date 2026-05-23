use std::path::Path;

use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub database_url: String,
    pub log: LogConfig,
    pub web: imkitchen_web_server::Config,
    pub admin: imkitchen_admin_server::Config,
}

#[derive(Debug, Deserialize)]
pub struct LogConfig {
    pub level: String,
    pub json: bool,
}

pub fn load(file: Option<&Path>) -> Result<AppConfig, ConfigError> {
    let mut builder = Config::builder()
        .set_default("database_url", "sqlite://imkitchen.db")?
        .set_default("log.level", "info,sqlx=warn,tower_http=info")?
        .set_default("log.json", false)?
        .set_default("web.port", 3000)?
        .set_default("web.timeout_secs", 5)?
        .set_default("web.body_limit_bytes", 1_048_576)?
        .set_default("admin.port", 3001)?
        .set_default("admin.timeout_secs", 5)?
        .set_default("admin.body_limit_bytes", 1_048_576)?;

    if let Some(path) = file {
        builder = builder.add_source(File::from(path).required(false));
    }

    builder
        .add_source(Environment::with_prefix("imkitchen").separator("__"))
        .build()?
        .try_deserialize()
}
