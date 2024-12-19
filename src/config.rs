use serde::Deserialize;

use crate::prepatch::PrePatch;

#[derive(Debug, Deserialize)]
pub struct Configuration {
    pub prepatches: Vec<PrePatch>,
}

pub fn config_from_env() -> Option<Configuration> {
    let raw = std::fs::read_to_string("everdiff.config.yaml").ok()?;
    println!("Loaded configuration...");
    serde_yaml::from_str(&raw)
        .inspect_err(|err| {
            println!("Failed to deserizlie config: {err:?}");
        })
        .ok()
}
