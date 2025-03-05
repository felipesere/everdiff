use std::cmp::Ordering;
use std::{collections::BTreeMap, fmt::Display};

use crate::diff::{ArrayOrdering, Difference as Diff};
use crate::identifier::IdentifierFn;
use crate::YamlSource;

#[derive(Debug)]
pub struct MatchingDocs {
    key: DocKey,
    left: usize,
    right: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MissingDoc {
    pub key: DocKey,
    pub left: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct AdditionalDoc {
    pub key: DocKey,
    pub right: usize,
}

pub struct Context {
    identifier: IdentifierFn,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("doc_identifier", &"a fn")
            .finish()
    }
}

impl Context {
    pub fn new_with_doc_identifier(identifier: IdentifierFn) -> Self {
        Context { identifier }
    }
}

fn matching_docs<F: Fn(usize, &YamlSource) -> Option<DocKey> + ?Sized>(
    lefts: &[YamlSource],
    rights: &[YamlSource],
    extract: &F,
) -> (Vec<MatchingDocs>, Vec<MissingDoc>, Vec<AdditionalDoc>) {
    let mut seen_left_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
    let mut seen_right_docs: BTreeMap<DocKey, usize> = BTreeMap::new();
    let mut matches = Vec::new();
    let mut missing_docs = Vec::new();
    let mut added_docs: Vec<AdditionalDoc> = Vec::new();

    let mut last_idx_used_on_right = 0_usize;
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
/// While the original file path is stored, it won't be used when doing Eq, Ord, or Hash
/// A common use case is to for example grab
/// * apiVersion
/// * kind
/// * metadata.name
///
/// from a Kubernetes resource to diff
#[derive(Debug, Clone, Eq)]
pub struct DocKey {
    src_file: camino::Utf8PathBuf,
    fields: BTreeMap<String, Option<String>>,
}

impl DocKey {
    pub fn pretty_print(&self) -> String {
        use comfy_table::modifiers::UTF8_ROUND_CORNERS;
        use comfy_table::presets::UTF8_FULL;

        let mut t = comfy_table::Table::new();
        t.load_preset(UTF8_FULL).apply_modifier(UTF8_ROUND_CORNERS);
        for (k, v) in self.fields.clone() {
            t.add_row(vec![k, v.unwrap_or_else(|| "∅".to_string())]);
        }

        format!("{t}")
    }
}

impl PartialOrd for DocKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::hash::Hash for DocKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fields.hash(state);
    }
}

impl Ord for DocKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.fields.cmp(&other.fields)
    }
}

impl PartialEq for DocKey {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}

impl DocKey {
    pub fn new(src_file: camino::Utf8PathBuf, fields: BTreeMap<String, Option<String>>) -> Self {
        DocKey { src_file, fields }
    }
}

impl Display for DocKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("file: {}\n", &self.src_file))?;
        for (k, optval) in &self.fields {
            if let Some(v) = &optval {
                f.write_fmt(format_args!("{k} → {v}\n"))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum DocDifference {
    Addition(AdditionalDoc),
    Missing(MissingDoc),
    Changed {
        key: DocKey,
        left_doc_idx: usize,
        right_doc_idx: usize,
        differences: Vec<Diff>,
    },
}

impl PartialOrd for DocDifference {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DocDifference {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (
                DocDifference::Addition(AdditionalDoc { key, .. }),
                DocDifference::Addition(AdditionalDoc { key: other_key, .. }),
            ) => key.cmp(other_key),
            (
                DocDifference::Missing(MissingDoc { key, .. }),
                DocDifference::Missing(MissingDoc { key: other_key, .. }),
            ) => key.cmp(other_key),
            (DocDifference::Changed { key, .. }, DocDifference::Changed { key: other_key, .. }) => {
                key.cmp(other_key)
            }
            (DocDifference::Addition(_), _) => Ordering::Less,
            (DocDifference::Changed { .. }, _) => Ordering::Greater,
            (DocDifference::Missing(_), DocDifference::Addition(_)) => Ordering::Greater,
            (DocDifference::Missing(_), DocDifference::Changed { .. }) => Ordering::Less,
        }
    }
}

pub fn diff(ctx: &Context, lefts: &[YamlSource], rights: &[YamlSource]) -> Vec<DocDifference> {
    let (matches, missing, added) = matching_docs(lefts, rights, &ctx.identifier);

    let mut differences = Vec::new();
    for MatchingDocs { key, left, right } in matches {
        let left_doc = &lefts[left].yaml;
        let right_doc = &rights[right].yaml;
        let mut diff_context = crate::diff::Context::new();
        diff_context.array_ordering = ArrayOrdering::Dynamic;

        let diffs = crate::diff::diff(diff_context, left_doc, right_doc);
        if !diffs.is_empty() {
            differences.push(DocDifference::Changed {
                key,
                left_doc_idx: left,
                right_doc_idx: right,
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
    use std::{collections::BTreeMap, str::FromStr};

    use expect_test::expect;
    use pretty_assertions::assert_eq;
    use saphyr::MarkedYaml;

    use crate::{
        multidoc::{diff, Context, DocKey},
        YamlSource,
    };
    use indoc::indoc;

    pub fn docs(raw: &str) -> Vec<YamlSource> {
        let docs = MarkedYaml::load_from_str(raw).expect("valid yaml");

        docs.into_iter()
            .map(|yaml| YamlSource {
                file: camino::Utf8PathBuf::from_str("/foo/bar/baz.yaml").unwrap(),
                yaml,
                content: raw.to_string(),
            })
            .collect()
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
          namespace: ns
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
          namespace: ns
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

        let ctx = Context::new_with_doc_identifier(crate::identifier::kubernetes::names());
        let differences = diff(&ctx, &left, &right);

        expect![[r#"
            [
                Changed {
                    key: DocKey {
                        src_file: "/foo/bar/baz.yaml",
                        fields: {
                            "metadata.name": Some(
                                "bravo",
                            ),
                            "metadata.namespace": None,
                        },
                    },
                    left_doc_idx: 0,
                    right_doc_idx: 1,
                    differences: [
                        Changed {
                            path: Path(
                                [
                                    Field(
                                        String(
                                            "spec",
                                        ),
                                    ),
                                    Field(
                                        String(
                                            "color",
                                        ),
                                    ),
                                ],
                            ),
                            left: MarkedYaml {
                                span: Span {
                                    start: Marker {
                                        index: 43,
                                        line: 5,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 49,
                                        line: 5,
                                        col: 15,
                                    },
                                },
                                data: String(
                                    "yellow",
                                ),
                            },
                            right: MarkedYaml {
                                span: Span {
                                    start: Marker {
                                        index: 109,
                                        line: 12,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 113,
                                        line: 12,
                                        col: 13,
                                    },
                                },
                                data: String(
                                    "blue",
                                ),
                            },
                        },
                    ],
                },
                Changed {
                    key: DocKey {
                        src_file: "/foo/bar/baz.yaml",
                        fields: {
                            "metadata.name": Some(
                                "alpha",
                            ),
                            "metadata.namespace": Some(
                                "ns",
                            ),
                        },
                    },
                    left_doc_idx: 1,
                    right_doc_idx: 0,
                    differences: [
                        Changed {
                            path: Path(
                                [
                                    Field(
                                        String(
                                            "spec",
                                        ),
                                    ),
                                    Field(
                                        String(
                                            "thing",
                                        ),
                                    ),
                                ],
                            ),
                            left: MarkedYaml {
                                span: Span {
                                    start: Marker {
                                        index: 113,
                                        line: 12,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 115,
                                        line: 12,
                                        col: 11,
                                    },
                                },
                                data: Integer(
                                    12,
                                ),
                            },
                            right: MarkedYaml {
                                span: Span {
                                    start: Marker {
                                        index: 59,
                                        line: 6,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 61,
                                        line: 6,
                                        col: 11,
                                    },
                                },
                                data: Integer(
                                    24,
                                ),
                            },
                        },
                    ],
                },
                Missing(
                    MissingDoc {
                        key: DocKey {
                            src_file: "/foo/bar/baz.yaml",
                            fields: {
                                "metadata.name": Some(
                                    "charlie",
                                ),
                                "metadata.namespace": None,
                            },
                        },
                        left: 2,
                    },
                ),
                Addition(
                    AdditionalDoc {
                        key: DocKey {
                            src_file: "/foo/bar/baz.yaml",
                            fields: {
                                "metadata.name": Some(
                                    "delta",
                                ),
                                "metadata.namespace": None,
                            },
                        },
                        right: 2,
                    },
                ),
            ]
        "#]]
        .assert_debug_eq(&differences);
    }

    #[test]
    fn display_dockey() {
        let key = DocKey::new(
            camino::Utf8PathBuf::from_str(r#"/foo/bar/baz.yaml"#).unwrap(),
            BTreeMap::from([
                ("api_version".to_string(), Some("bar".to_string())),
                ("metadata.name".to_string(), Some("foo".to_string())),
            ]),
        );
        assert_eq!(
            key.to_string(),
            indoc! {r#"
            file: /foo/bar/baz.yaml
            api_version → bar
            metadata.name → foo
        "#}
        );
    }
}
