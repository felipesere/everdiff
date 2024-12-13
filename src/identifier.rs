use crate::multidoc::DocKey;
use std::collections::BTreeMap;

/// Fn that identifies a document by inspecting keys
pub type IdentifierFn = Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>>;

/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> IdentifierFn {
    Box::new(|idx, _| {
        Some(DocKey::from(BTreeMap::from([(
            "idx".to_string(),
            Some(idx.to_string()),
        )])))
    })
}

pub mod kubernetes {
    use super::*;
    use std::collections::BTreeMap;

    fn string_of(node: Option<&serde_yaml::Value>) -> Option<String> {
        node?.as_str().map(String::from)
    }

    /// Keys to identify immutable kinds
    pub fn gvk() -> IdentifierFn {
        Box::new(|_, doc| {
            let api_version = string_of(doc.get("apiVersion"));
            let kind = string_of(doc.get("kind"));
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));

            Some(DocKey::from(BTreeMap::from([
                ("api_version".to_string(), api_version),
                ("kind".to_string(), kind),
                ("metadata.name".to_string(), name),
            ])))
        })
    }

    /// Keys used to find renamed kinds
    pub fn names() -> IdentifierFn {
        Box::new(|_, doc| {
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));
            let namespace = string_of(doc.get("metadata")?.get("namespace"));
            Some(DocKey::from(BTreeMap::from([
                ("metadata.name".to_string(), name),
                ("metadata.namespace".to_string(), namespace),
            ])))
        })
    }
}
