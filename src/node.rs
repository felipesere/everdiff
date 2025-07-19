use saphyr::{AnnotatedMapping, Indexable, MarkedYamlOwned};
use saphyr_parser::Span;

use crate::path::Path;

pub fn sub_mapping(original: &MarkedYamlOwned, target: &Path) -> Option<MarkedYamlOwned> {
    let (key, value) = node_and_key(original, target)?;
    let start = key.span.start;
    let end = value.span.end;

    let mut m = AnnotatedMapping::new();
    m.insert(key, value);

    Some(MarkedYamlOwned {
        span: Span { start, end },
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

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use saphyr::{LoadableYamlNode, MarkedYamlOwned};

    use crate::path::Path;

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
        saphyr::YamlEmitter::new(&mut buf).dump(&outcome).unwrap();

        expect![[r#"
            ---
            target:
              name: Foo
              value: bar
        "#]]
        .assert_eq(&buf);
    }
}
