use everdiff_multidoc::{Fields, IdentifierFn};
use saphyr::MarkedYamlOwned;

/// Naively assume that a document is identified by its index in the document.
/// This effectively means that documents are diffed pair-wise in the
/// order they show up in the YAML
pub fn by_index() -> IdentifierFn {
    Box::new(|idx, _source| Some(Fields::default().with_idx(idx)))
}

fn string_of(node: Option<&MarkedYamlOwned>) -> Option<String> {
    node?.data.as_str().map(String::from)
}

pub mod talos {
    use super::*;
    use saphyr::SafelyIndex;

    pub fn documents() -> IdentifierFn {
        Box::new(|_idx, source| {
            let doc = &source.yaml;
            let api_version = string_of(doc.get("apiVersion"));
            let version = string_of(doc.get("version"));
            let kind = string_of(doc.get("kind"));
            let machine = doc.get("machine");
            let cluster = doc.get("cluster");
            let name = string_of(doc.get("name"));

            let mut fields = Fields::default().with("kind", kind);

            if let Some(api_version) = api_version {
                fields.insert("apiVersion", api_version);
            }

            if let Some(version) = version {
                fields.insert("version", version);
            }

            if machine.is_some() {
                fields.insert("machine", "present".to_string());
            }

            if cluster.is_some() {
                fields.insert("cluster", "present".to_string());
            }

            if name.is_some() {
                fields.insert("name".to_string(), name);
            }

            Some(fields)
        })
    }
}

pub mod kubernetes {
    use super::*;
    use saphyr::SafelyIndex;

    /// Keys to identify immutable kinds
    pub fn gvk() -> IdentifierFn {
        Box::new(|_idx, source| {
            let doc = &source.yaml;
            let api_version = string_of(doc.get("apiVersion"));
            let kind = string_of(doc.get("kind"));
            // TODO: don't bail on missing metadata
            let name = string_of(doc.get("metadata")?.get("name"));

            let fields = Fields::default()
                .with("api_version", api_version)
                .with("kind", kind)
                .with("metadata.name", name);
            Some(fields)
        })
    }
}
