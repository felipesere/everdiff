use std::collections::BTreeMap;

use crate::{YamlSource, multidoc::DocKey};

/// Fn that identifies a document by inspecting keys
pub type IdentifierFn = Box<dyn Fn(usize, &YamlSource) -> Option<DocKey>>;

/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> IdentifierFn {
    Box::new(|idx, source| {
        Some(DocKey::new(
            source.file.clone(),
            BTreeMap::from([("idx".to_string(), Some(idx.to_string()))]),
        ))
    })
}

pub mod kubernetes {
    use saphyr::{Indexable, MarkedYamlOwned};

    use super::*;
    use std::collections::BTreeMap;

    fn string_of(node: Option<&MarkedYamlOwned>) -> Option<String> {
        node?.data.as_str().map(String::from)
    }

    /// Keys to identify immutable kinds
    pub fn gvk() -> IdentifierFn {
        Box::new(|_, source| {
            let doc = &source.yaml;
            let api_version = string_of(doc.get("apiVersion"));
            let kind = string_of(doc.get("kind"));
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));

            Some(DocKey::new(
                source.file.clone(),
                BTreeMap::from([
                    ("api_version".to_string(), api_version),
                    ("kind".to_string(), kind),
                    ("metadata.name".to_string(), name),
                ]),
            ))
        })
    }

    /// Keys used to find renamed kinds
    pub fn names() -> IdentifierFn {
        Box::new(|_, source| {
            let doc = &source.yaml;
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));
            let namespace = string_of(doc.get("metadata")?.get("namespace"));
            Some(DocKey::new(
                source.file.clone(),
                BTreeMap::from([
                    ("metadata.name".to_string(), name),
                    ("metadata.namespace".to_string(), namespace),
                ]),
            ))
        })
    }
}
