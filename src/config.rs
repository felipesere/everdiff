use serde::Deserialize;

use crate::prepatch::PrePatch;

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub prepatches: Vec<PrePatch>,
}

pub fn config_from_env() -> Option<Configuration> {
    let raw = std::fs::read_to_string("everdiff.config.yaml").ok()?;
    serde_yaml::from_str(&raw).ok()
}
