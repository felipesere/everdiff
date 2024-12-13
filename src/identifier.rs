use crate::multidoc::DocKey;
use std::collections::BTreeMap;

/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>> {
    Box::new(|idx, _| {
        Some(DocKey::from(BTreeMap::from([(
            "idx".to_string(),
            idx.to_string(),
        )])))
    })
}

pub mod kubernetes {
    use super::*;
    use std::collections::BTreeMap;

    fn string_of(node: &serde_yaml::Value) -> String {
        node.as_str().map(String::from).unwrap()
    }

    pub fn apiversion_resource_name() -> Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>> {
        Box::new(|_, doc| {
            let api_version = string_of(doc.get("apiVersion")?);
            let kind = string_of(doc.get("kind")?);
            let name = string_of(doc.get("metadata")?.get("name")?);

            Some(DocKey::from(BTreeMap::from([
                ("api_version".to_string(), api_version),
                ("kind".to_string(), kind),
                ("metadata.name".to_string(), name),
            ])))
        })
    }

    pub fn metadata_name() -> Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>> {
        Box::new(|_, doc| {
            doc.get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .map(|name| {
                    DocKey::from(BTreeMap::from([(
                        "metadata.name".to_string(),
                        name.to_string(),
                    )]))
                })
        })
    }
}
