use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Shortcut {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Category {
    pub name: String,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub categories: Vec<Category>,
}

pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}
