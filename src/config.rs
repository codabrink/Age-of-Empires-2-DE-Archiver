use anyhow::Result;
use serde::Deserialize;
use std::fs::read_to_string;

#[derive(Deserialize)]
pub struct Config {
    pub goldberg: Goldberg,
    pub aoe2: AoE2,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_str = read_to_string("config.toml")?;
        Ok(toml::from_str(&config_str)?)
    }
}

#[derive(Deserialize)]
pub struct Goldberg {
    pub download_url: String,
}

#[derive(Deserialize)]
pub struct AoE2 {
    pub steam_folder: String,
    pub gh_user: String,
    pub gh_repo: String,
}
