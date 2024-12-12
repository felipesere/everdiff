use std::collections::BTreeMap;

use crate::{identifier, Difference as Diff};

#[derive(Debug)]
pub struct MatchingDocs {
    key: DocKey,
    left: usize,
    right: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MissingDoc {
    key: DocKey,
    left: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct AdditionalDoc {
    key: DocKey,
    right: usize,
}

pub struct Context {
    doc_identifier: Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("doc_identifier", &"a fn")
            .finish()
    }
}

impl Context {
    pub fn new() -> Self {
        Context {
            doc_identifier: Box::new(identifier::by_index()),
        }
    }

    pub fn new_with_doc_identifier(
        identifier: Box<dyn Fn(usize, &serde_yaml::Value) -> Option<DocKey>>,
    ) -> Self {
        Context {
            doc_identifier: identifier,
        }
    }
}

fn matching_docs<F: Fn(usize, &serde_yaml::Value) -> Option<DocKey>>(
    lefts: &[serde_yaml::Value],
    rights: &[serde_yaml::Value],
    extract: F,
) -> (Vec<MatchingDocs>, Vec<MissingDoc>, Vec<AdditionalDoc>) {
    let mut seen_left_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
    let mut seen_right_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
    let mut matches = Vec::new();
    let mut missing_docs = Vec::new();
    let mut added_docs: Vec<AdditionalDoc> = Vec::new();

    let mut last_idx_used_on_right = 0usize;
    'comparing_left_docs: for (idx, doc) in lefts.iter().enumerate() {
        if let Some(key) = extract(idx, doc) {
            seen_left_docs.insert(key.clone(), idx);
            if let Some(right) = seen_right_docs.get(&key) {
                matches.push(MatchingDocs {
                    key,
                    left: idx,
                    right: *right,
                });
                continue 'comparing_left_docs;
            }

            for (right, doc) in rights.iter().enumerate().skip(last_idx_used_on_right) {
                if let Some(right_key) = extract(right, doc) {
                    seen_right_docs.insert(right_key.clone(), idx);
                    if right_key == key {
                        matches.push(MatchingDocs {
                            key,
                            left: idx,
                            right,
                        });
                        last_idx_used_on_right = right;
                        continue 'comparing_left_docs;
                    }
                }
            }
            // ...we've gone through all the docs on the "right" without finding a match, it must
            // be missing
            missing_docs.push(MissingDoc { key, left: idx })
        }
    }
    // let's go over all docs we've seen on the right and check which ones don't exist on the left
    for (key, right) in seen_right_docs {
        if seen_left_docs.contains_key(&key) {
            continue;
        }
        added_docs.push(AdditionalDoc { key, right })
    }

    (matches, missing_docs, added_docs)
}

/// Newtype used to identify a document.
/// Two Documents that produce the same `DocKey` will be diffed
/// against each other.
/// A common use case is to for example grab
/// * apiVersion
/// * kind
/// * metadata.name
///
/// from a Kubernetes resource to diff
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct DocKey(BTreeMap<String, String>);

impl From<BTreeMap<String, String>> for DocKey {
    fn from(value: BTreeMap<String, String>) -> Self {
        Self(value)
    }
}

/// Newtype around a usize to index into the collection of Documents
// #[derive(Debug, Eq, PartialEq, Clone, Copy)]
//struct DocIdx(usize);

#[derive(Debug, Eq, PartialEq)]
pub enum DocDifference {
    Addition(AdditionalDoc),
    Missing(MissingDoc),
    Changed {
        key: DocKey,
        left_doc: usize,
        right_doc: usize,
        differences: Vec<Diff>,
    },
}

pub fn diff(
    ctx: Context,
    lefts: &[serde_yaml::Value],
    rights: &[serde_yaml::Value],
) -> Vec<DocDifference> {
    let (matches, missing, added) = matching_docs(lefts, rights, ctx.doc_identifier);

    let mut differences = Vec::new();
    for MatchingDocs { key, left, right } in matches {
        let left_doc = &lefts[left];
        let right_doc = &rights[right];
        let diffs = super::diff(super::Context::new(), left_doc, right_doc);
        if !diffs.is_empty() {
            differences.push(DocDifference::Changed {
                key,
                left_doc: left,
                right_doc: right,
                differences: diffs,
            })
        }
    }
    for m in missing {
        differences.push(DocDifference::Missing(m));
    }
    for a in added {
        differences.push(DocDifference::Addition(a));
    }
    differences
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    use crate::{
        multidoc::{diff, AdditionalDoc, Context, DocDifference, DocKey, MissingDoc},
        Difference, Path,
    };
    use indoc::indoc;
    use serde::Deserialize;

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
        ---
        metadata:
          name: charlie
        spec:
          wheels: 6
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
        ---
        metadata:
          name: delta
        spec:
          size: xl
        ...
        "#});

        let ctx = Context::new_with_doc_identifier(Box::new(
            crate::identifier::kubernetes::by_api_namespace_name(),
        ));
        let differences = diff(ctx, &left, &right);

        assert_eq!(
            differences,
            vec![
                DocDifference::Changed {
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
                DocDifference::Changed {
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
                },
                DocDifference::Missing(MissingDoc {
                    key: DocKey(BTreeMap::from([(
                        "metadata.name".to_string(),
                        "charlie".to_string()
                    )])),
                    left: 2,
                }),
                DocDifference::Addition(AdditionalDoc {
                    key: DocKey(BTreeMap::from([(
                        "metadata.name".to_string(),
                        "delta".to_string()
                    )])),
                    right: 2,
                }),
            ]
        )
    }
}
