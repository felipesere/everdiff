use std::collections::BTreeMap;

use everdiff_multidoc::{Fields, IdentifierFn};
use saphyr::MarkedYamlOwned;

/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> IdentifierFn {
    Box::new(|idx, _source| {
        Some(Fields(BTreeMap::from([(
            "idx".to_string(),
            Some(idx.to_string()),
        )])))
    })
}

fn string_of(node: Option<&MarkedYamlOwned>) -> Option<String> {
    node?.data.as_str().map(String::from)
}

pub mod talos {
    use super::*;
    use saphyr::SafelyIndex;
    use std::collections::BTreeMap;

    pub fn documents() -> IdentifierFn {
        Box::new(|idx, source| {
            let doc = &source.yaml;
            let api_version = string_of(doc.get("apiVersion"));
            let version = string_of(doc.get("version"));
            let kind = string_of(doc.get("kind"));
            let machine = doc.get("machine");
            let cluster = doc.get("cluster");
            let name = string_of(doc.get("name"));

            let mut fields = BTreeMap::from([("kind".to_string(), kind)]);

            if let Some(api_version) = api_version {
                fields.insert("apiVersion".to_string(), Some(api_version));
            }

            if let Some(version) = version {
                fields.insert("version".to_string(), Some(version));
            }

            if machine.is_some() {
                fields.insert("machine".to_string(), Some("present".to_string()));
            }

            if cluster.is_some() {
                fields.insert("cluster".to_string(), Some("present".to_string()));
            }

            if let Some(name) = name {
                fields.insert("name".to_string(), Some(name));
            }

            dbg!(&source.file);
            dbg!(&idx);
            dbg!(&fields);

            Some(Fields(fields))
        })
    }
}

pub mod kubernetes {
    use super::*;
    use saphyr::SafelyIndex;
    use std::collections::BTreeMap;

    /// Keys to identify immutable kinds
    pub fn gvk() -> IdentifierFn {
        Box::new(|_idx, source| {
            let doc = &source.yaml;
            let api_version = string_of(doc.get("apiVersion"));
            let kind = string_of(doc.get("kind"));
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));

            Some(Fields(BTreeMap::from([
                ("api_version".to_string(), api_version),
                ("kind".to_string(), kind),
                ("metadata.name".to_string(), name),
            ])))
        })
    }
}
