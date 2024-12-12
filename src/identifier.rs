use std::collections::BTreeMap;

use crate::multidoc::DocKey;
/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> impl Fn(usize, &serde_yaml::Value) -> Option<DocKey> {
    |idx, _| {
        Some(DocKey::from(BTreeMap::from([(
            "idx".to_string(),
            idx.to_string(),
        )])))
    }
}

pub mod kubernetes {
    use super::*;
    use std::collections::BTreeMap;

    pub fn by_api_namespace_name() -> impl Fn(usize, &serde_yaml::Value) -> Option<DocKey> {
        |_, doc| {
            doc.get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .map(|name| {
                    DocKey::from(BTreeMap::from([(
                        "metadata.name".to_string(),
                        name.to_string(),
                    )]))
                })
        }
    }
}
