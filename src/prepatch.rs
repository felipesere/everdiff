use json_patch::PatchOperation;
use jsonptr::resolve::ResolveError;
use serde::Deserialize;
use serde_yaml::Value;

use crate::YamlSource;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Value to patch not found")]
    ValueNotFoundAtPath,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrePatch {
    name: Option<String>,
    document_like: Option<serde_yaml::Value>,
    patches: json_patch::Patch,
}

impl PrePatch {
    pub fn apply_to(&self, documents: &mut Vec<YamlSource>) -> Result<(), Error> {
        for doc in documents {
            if let Some(doc_matcher) = &self.document_like {
                if !document_matches(doc_matcher, &doc.yaml) {
                    continue;
                }
            }
            apply_patch(&self.patches, &mut doc.yaml)?;
        }

        Ok(())
    }
}

// Shamelessly stolen from jsontr::Pointer.
// It comes from a `Resolve` trait which is implemented for `serde_json::Value` and TOML
// but sadly not for `serde_yaml::Value`.
/// Get mutable access to the Value that `ptr` points at within `value`.
fn resolve_mut<'a>(
    mut value: &'a mut serde_yaml::Value,
    mut ptr: &jsonptr::Pointer,
) -> Result<&'a mut serde_yaml::Value, anyhow::Error> {
    let mut offset = 0;
    while let Some((token, rem)) = ptr.split_front() {
        let tok_len = token.encoded().len();
        ptr = rem;
        value = match value {
            Value::Sequence(v) => {
                let idx = token
                    .to_index()
                    .map_err(|source| ResolveError::FailedToParseIndex { offset, source })?
                    .for_len(v.len())
                    .map_err(|source| ResolveError::OutOfBounds { offset, source })?;
                Ok(v.get_mut(idx).unwrap())
            }

            Value::Mapping(v) => v
                .get_mut(token.decoded().as_ref())
                .ok_or(ResolveError::NotFound { offset }),
            // found a leaf node but the pointer hasn't been exhausted
            _ => Err(ResolveError::Unreachable { offset }),
        }?;
        offset += 1 + tok_len;
    }
    Ok(value)
}

fn apply_patch(patches: &json_patch::Patch, doc: &mut Value) -> Result<(), Error> {
    for p in patches.iter() {
        match p {
            PatchOperation::Replace(r) => {
                if let Ok(v) = resolve_mut(doc, &r.path) {
                    let replacement: serde_yaml::Value =
                        serde_json::from_value(r.value.clone()).unwrap();
                    *v = replacement;
                } else {
                    return Err(Error::ValueNotFoundAtPath);
                }
            }
            PatchOperation::Add(a) => {
                if let Some((path, field)) = a.path.split_back() {
                    if let Ok(v) = resolve_mut(doc, path) {
                        if let Some(m) = v.as_mapping_mut() {
                            let new_value: serde_yaml::Value =
                                serde_json::from_value(a.value.clone()).unwrap();
                            let key = field.to_string().into();
                            m.insert(key, new_value);
                        };
                    } else {
                        return Err(Error::ValueNotFoundAtPath);
                    }
                }
            }
            _ => unimplemented!("We only currently support add & replace"),
        }
    }
    Ok(())
}

fn document_matches(document_like: &Value, doc: &Value) -> bool {
    match (document_like, doc) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Sequence(required), Value::Sequence(available)) => {
            for (r, a) in required.iter().zip(available.iter()) {
                if !document_matches(r, a) {
                    return false;
                }
            }
            true
        }
        (Value::Mapping(required), Value::Mapping(available)) => {
            for (key, value) in required {
                let Some(other_value) = available.get(key) else {
                    return false;
                };
                if !document_matches(value, other_value) {
                    return false;
                }
            }

            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use indoc::indoc;

    use crate::YamlSource;

    use super::PrePatch;

    #[test]
    fn can_read_a_patch_statement() {
        let raw = indoc! {r#"
            name: rename network policy to match chart convention
            # documentIndex: 4
            documentLike:
              kind: NetworkPolicy
              metadata:
                name: flux-engine-steam
            patches:
              - op: replace
                path: "/metadata/name"
                value: "flux"
        "#};

        let pp: PrePatch = serde_yaml::from_str(raw).unwrap();
    }

    #[test]
    fn replaces_the_name_of_the_networkpolicy_to_match() {
        let raw_docs = indoc! {r#"
        ---
        kind: NetworkPolicy
        metadata:
          name:  flux-engine-steam
        ---
        kind: NetworkPolicy
        metadata:
          name:  the-other-one
        "#};

        let mut documents = docs(raw_docs);
        let pp: PrePatch = serde_yaml::from_str(indoc! {r#"
            name: rename network policy to match chart convention
            # documentIndex: 4
            documentLike:
              kind: NetworkPolicy
              metadata:
                name: flux-engine-steam
            patches:
              - op: replace
                path: "/metadata/name"
                value: "flux"
        "#})
        .unwrap();

        pp.apply_to(&mut documents);

        let outcome = serialize(&documents);
        expect![[r#"
            kind: NetworkPolicy
            metadata:
              name: flux
            ---
            kind: NetworkPolicy
            metadata:
              name: the-other-one
        "#]]
        .assert_eq(&outcome);
    }

    #[test]
    fn adds_the_namespace_to_all_documents() {
        let raw_docs = indoc! {r#"
        ---
        kind: NetworkPolicy
        metadata:
          name: flux-engine-steam
        ---
        kind: Deployment
        metadata:
          name: the-other-one
        "#};

        let mut documents = docs(raw_docs);
        let pp: PrePatch = serde_yaml::from_str(indoc! {r#"
            name: Add the namespace everywhere
            patches:
              - op: add
                path: "/metadata/namespace"
                value: "core"
        "#})
        .unwrap();

        pp.apply_to(&mut documents);

        let outcome = serialize(&documents);
        expect![[r#"
            kind: NetworkPolicy
            metadata:
              name: flux-engine-steam
              namespace: core
            ---
            kind: Deployment
            metadata:
              name: the-other-one
              namespace: core
        "#]]
        .assert_eq(&outcome);
    }

    pub fn docs(raw: &str) -> Vec<YamlSource> {
        use serde::Deserialize;
        use serde_yaml::{Deserializer, Value};

        let mut docs = Vec::new();
        for document in Deserializer::from_str(raw) {
            let yaml = Value::deserialize(document).unwrap();
            docs.push(YamlSource {
                file: camino::Utf8PathBuf::new(),
                yaml,
            });
        }
        docs
    }

    pub fn serialize(docs: &[YamlSource]) -> String {
        use serde::Serialize;

        let mut serializer = serde_yaml::Serializer::new(Vec::new());
        for doc in docs {
            doc.yaml.serialize(&mut serializer).unwrap();
        }
        String::from_utf8(serializer.into_inner().unwrap()).unwrap()
    }
}
