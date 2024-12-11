use std::collections::BTreeMap;

use crate::Difference as Diff;

pub struct Context {}

#[derive(Debug)]
struct MatchingDocs {
    key: DocKey,
    left: usize,
    right: usize,
}

impl Context {
    pub fn new() -> Self {
        Context {}
    }

    fn matching_docs<F: Fn(usize, &serde_yaml::Value) -> Option<DocKey>>(
        &self,
        lefts: &[serde_yaml::Value],
        rights: &[serde_yaml::Value],
        extract: F,
    ) -> (
        Vec<MatchingDocs>,
        Vec<serde_yaml::Value>,
        Vec<serde_yaml::Value>,
    ) {
        // let mut lefty_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
        let mut righty_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
        let mut matches = Vec::new();

        let mut last_idx_used_on_right = 0usize;
        for (idx, doc) in lefts.iter().enumerate() {
            if let Some(key) = extract(idx, doc) {
                // lefty_docs.insert(key.clone(), idx);
                if let Some(right) = righty_docs.get(&key) {
                    matches.push(MatchingDocs {
                        key,
                        left: idx,
                        right: *right,
                    });
                    continue;
                }

                for right in last_idx_used_on_right..rights.len() {
                    let doc = &rights[right];
                    if let Some(right_key) = extract(right, doc) {
                        righty_docs.insert(right_key.clone(), idx);
                        if right_key == key {
                            matches.push(MatchingDocs {
                                key,
                                left: idx,
                                right,
                            });
                            last_idx_used_on_right = right;
                            break;
                        }
                    }
                }
            }
        }
        dbg!(&matches);

        // Naive one should go number by number...
        (matches, Vec::new(), Vec::new())
    }
}

/// Newtype used to identify a document.
/// Two Documents that produce the same `DocKey` will be diffed
/// against each other.
/// A common use case is to for example grab
/// * apiVersion
/// * kind
/// * metadata.name
/// from a Kubernetes resource to diff
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct DocKey(BTreeMap<String, String>);

/// Newtype around a usize to index into the collection of Documents
// #[derive(Debug, Eq, PartialEq, Clone, Copy)]
//struct DocIdx(usize);

#[derive(Debug, Eq, PartialEq)]
pub enum DocDifference {
    AdditionalDocument(usize),
    MissingDocument(usize),
    ChangedDocument {
        key: DocKey,
        left_doc: usize,
        right_doc: usize,
        differences: Vec<Diff>,
    },
}
pub fn diff(
    ctx: Context,
    lefts: &Vec<serde_yaml::Value>,
    rights: &Vec<serde_yaml::Value>,
) -> Vec<DocDifference> {
    let (matches, _missing, _added) = ctx.matching_docs(lefts, rights, |_, doc| {
        doc.get("metadata")
            .and_then(|m| m.get("name"))
            .and_then(|n| n.as_str())
            .map(|name| {
                DocKey(BTreeMap::from([(
                    "metadata.name".to_string(),
                    name.to_string(),
                )]))
            })
    });
    // find 2 matching documents
    //
    let mut xs = Vec::new();
    for MatchingDocs { key, left, right } in matches {
        let left_doc = &lefts[left];
        let right_doc = &rights[right];
        let diffs = super::diff(super::Context::new(), left_doc, right_doc);
        if !diffs.is_empty() {
            xs.push(DocDifference::ChangedDocument {
                key,
                left_doc: left,
                right_doc: right,
                differences: diffs,
            })
        }
    }
    xs
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    use crate::{
        multidoc::{diff, DocDifference, DocKey},
        Difference, Path,
    };
    use indoc::indoc;
    use serde::Deserialize;

    use super::Context;

    pub fn docs(raw: &str) -> Vec<serde_yaml::Value> {
        let mut docs = Vec::new();
        for document in serde_yaml::Deserializer::from_str(raw) {
            let v = serde_yaml::Value::deserialize(document).unwrap();
            docs.push(v);
        }
        docs
    }

    #[test]
    fn two_documents_changed_out_of_order() {
        let left = docs(indoc! {r#"
        ---
        metadata:
          name: bravo
        spec:
          color: yellow
        ...
        ---
        metadata:
          name: alpha
        spec:
          thing: 12
        ...
        "#});

        let right = docs(indoc! {r#"
        ---
        metadata:
          name: alpha
        spec:
          thing: 24
        ...
        ---
        metadata:
          name: bravo
        spec:
          color: blue
        ...
        "#});

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![
                DocDifference::ChangedDocument {
                    key: DocKey(BTreeMap::from([(
                        "metadata.name".to_string(),
                        "bravo".to_string()
                    )])),
                    left_doc: 0,
                    right_doc: 1,
                    differences: vec![Difference::Changed {
                        path: Path::from_unchecked(vec![".".into(), "spec".into(), "color".into()]),
                        left: serde_yaml::Value::String("yellow".into()),
                        right: serde_yaml::Value::String("blue".into()),
                    }]
                },
                DocDifference::ChangedDocument {
                    key: DocKey(BTreeMap::from([(
                        "metadata.name".to_string(),
                        "alpha".to_string()
                    )])),
                    left_doc: 1,
                    right_doc: 0,
                    differences: vec![Difference::Changed {
                        path: Path::from_unchecked(vec![".".into(), "spec".into(), "thing".into()]),
                        left: serde_yaml::Value::Number(12.into()),
                        right: serde_yaml::Value::Number(24.into()),
                    }]
                }
            ]
        )
    }
}
