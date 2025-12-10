use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Configuration {}

pub fn config_from_env() -> Option<Configuration> {
    let raw = std::fs::read_to_string("everdiff.config.yaml").ok()?;
    serde_saphyr::from_str(&raw)
        .inspect_err(|err| {
            println!("Failed to deserizlie config: {err:?}");
        })
        .ok()
}
