use crate::diff::{Item, string_value};
use crate::path::{Path, Segment};
use saphyr::{AnnotatedMapping, LoadableYamlNode, MarkedYamlOwned, SafelyIndex};

pub fn sub_mapping(original: &MarkedYamlOwned, target: &Path) -> Option<MarkedYamlOwned> {
    let (key, value) = node_and_key(original, target)?;
    let mut span = key.span;
    span.end = value.span.end;

    let mut m = AnnotatedMapping::new();
    m.insert(key, value);

    Some(MarkedYamlOwned {
        span,
        data: saphyr::YamlDataOwned::Mapping(m),
    })
}

pub fn node_and_key(
    yaml: &MarkedYamlOwned,
    path: &Path,
) -> Option<(MarkedYamlOwned, MarkedYamlOwned)> {
    let f = path.segments().first();

    let mut n = f.map(|f| f.as_yaml()).zip(Some(yaml.clone()));
    for p in path.segments() {
        n = n.and_then(|(_old_key, n)| {
            let mapping = n.data.as_mapping()?;
            mapping
                .get_key_value(&p.as_yaml())
                .map(|(a, b)| (a.clone(), b.clone()))
        });
    }
    n
}

pub fn node_in_2(yaml: &MarkedYamlOwned, path: &Path) -> Option<Item> {
    let Some((last, rest)) = path.segments().split_last() else {
        log::debug!("Very odd that the path was empty...");
        return None;
    };
    let mut n = yaml;
    for p in rest {
        match p {
            crate::path::Segment::Field(f) => {
                n = n.get(f.as_str())?;
            }
            crate::path::Segment::Index(nr) => {
                n = n.get(*nr)?;
            }
        }
    }
    match last {
        Segment::Field(f) => {
            let key = n
                .data
                .as_mapping()?
                .keys()
                .find(|key| key.data.as_str().is_some_and(|t| t == f))?;
            Some(Item::KV {
                key: key.clone(),
                value: yaml.get(f.as_str()).cloned().unwrap(),
            })
        }
        Segment::Index(index) => {
            let value = n.data.as_sequence_get(*index)?;

            Some(Item::ArrayElement {
                index: (*index).try_into().ok()?,
                value: value.clone(),
            })
        }
    }
}

pub fn node_in<'y>(yaml: &'y MarkedYamlOwned, path: &Path) -> Option<&'y MarkedYamlOwned> {
    let mut n = Some(yaml);
    for p in path.segments() {
        match p {
            crate::path::Segment::Field(f) => {
                let v = n.and_then(|n| n.get(f.as_str()))?;
                n = Some(v);
            }
            crate::path::Segment::Index(nr) => {
                let v = n.and_then(|n| n.get(*nr))?;
                n = Some(v);
            }
        }
    }
    n
}

pub fn to_value(marked_yaml: &MarkedYamlOwned) -> saphyr::Yaml {
    use saphyr::{ScalarOwned, Yaml, YamlDataOwned};

    match &marked_yaml.data {
        YamlDataOwned::Representation(s, scalar_style, tag) => Yaml::Representation(
            std::borrow::Cow::Borrowed(s),
            *scalar_style,
            tag.as_ref().map(|t| std::borrow::Cow::Owned(t.clone())),
        ),
        YamlDataOwned::Value(ScalarOwned::Null) => Yaml::Value(saphyr::Scalar::Null),
        YamlDataOwned::Value(ScalarOwned::Boolean(b)) => Yaml::Value(saphyr::Scalar::Boolean(*b)),
        YamlDataOwned::Value(ScalarOwned::Integer(i)) => Yaml::Value(saphyr::Scalar::Integer(*i)),
        YamlDataOwned::Value(ScalarOwned::FloatingPoint(fp)) => {
            Yaml::Value(saphyr::Scalar::FloatingPoint(*fp))
        }
        YamlDataOwned::Value(ScalarOwned::String(s)) => Yaml::Value(saphyr::Scalar::String(
            std::borrow::Cow::Borrowed(s.as_str()),
        )),
        YamlDataOwned::Sequence(items) => Yaml::Sequence(items.iter().map(to_value).collect()),
        YamlDataOwned::Mapping(linked_hash_map) => Yaml::Mapping(
            linked_hash_map
                .iter()
                .map(|(key, value)| (to_value(key), to_value(value)))
                .collect(),
        ),
        YamlDataOwned::Tagged(tag, v) => {
            Yaml::Tagged(std::borrow::Cow::Owned(tag.clone()), Box::new(to_value(v)))
        }
        YamlDataOwned::Alias(a) => Yaml::Alias(*a),
        YamlDataOwned::BadValue => Yaml::BadValue,
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use saphyr::{LoadableYamlNode, MarkedYamlOwned};

    use crate::{node::to_value, path::Path};

    use super::sub_mapping;

    #[test]
    fn extract_mapping_from_another_mapping() {
        let yaml = MarkedYamlOwned::load_from_str(indoc::indoc!(
            r#"
        top:
          first: thing
          target:
            name: Foo
            value: bar
          last: true
        "#,
        ))
        .unwrap()
        .remove(0);

        let outcome = sub_mapping(&yaml, &Path::parse_str(".top.target")).unwrap();

        let mut buf = String::new();
        saphyr::YamlEmitter::new(&mut buf)
            .dump(&to_value(&outcome))
            .unwrap();

        expect![[r#"
            ---
            target:
              name: Foo
              value: bar"#]]
        .assert_eq(&buf);
    }
}
