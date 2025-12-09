use camino::Utf8PathBuf;
use saphyr::LoadableYamlNode;

use crate::snippet::Line;

// TODO: Should this live elsewhere?
#[derive(Debug, Clone)]
pub struct YamlSource {
    pub file: camino::Utf8PathBuf,
    pub yaml: saphyr::MarkedYamlOwned,
    pub content: String,
    pub index: usize,
    /// these numbers are based on the file itself.
    /// they do come from the parser, but carry on counting
    /// up across multiple docs within the same file
    /// and include leading empty lines
    pub start: usize,
    pub end: usize,
    // These are relative numbers of actual YAML conent. Trailing empty lines are not counted.
    // Unless something is funky, first line should always be Line(1)
    pub first_line: Line,
    pub last_line: Line,
}

pub fn read_doc(content: impl Into<String>, path: Utf8PathBuf) -> anyhow::Result<Vec<YamlSource>> {
    let content = content.into();
    let mut docs = Vec::new();
    let raw_docs: Vec<_> = content
        .clone()
        .split("---")
        .filter(|doc| !doc.is_empty())
        .map(|doc| doc.trim().to_string())
        .collect();

    let parsed_docs = saphyr::MarkedYamlOwned::load_from_str(&content)?;

    for (index, (document, content)) in parsed_docs.into_iter().zip(raw_docs).enumerate() {
        let start = document.span.start.line();
        let end = document.span.end.line();
        log::debug!("start: {start} and end {end}");

        let n = content
            .lines()
            .rev()
            // drop any trailing empty lines...
            .skip_while(|line| line.is_empty())
            .count();

        let first_line = Line::one();
        // the span ends when the indenation no longer matches, which is the line _after_ the the
        // last properly indented line
        let last_line = Line::new(n).unwrap();

        docs.push(YamlSource {
            file: path.clone(),
            yaml: document,
            start,
            end,
            first_line,
            last_line,
            content,
            index,
        });
    }
    Ok(docs)
}

impl YamlSource {
    pub fn lines(&self) -> Vec<&str> {
        self.content
            .lines()
            .skip_while(|line| *line == "---" || line.is_empty())
            .collect()
    }

    /// Turn the absolute, file-wide line number into one that
    /// is relative to the beginning of the document
    pub fn relative_line(&self, line: usize) -> Line {
        let start = self.start;
        log::debug!(
            "the start of the document is on absolute line {start}, and we are checking for line {line}",
        );
        // If the line we ask for is literally the start, this would be `start - start + 1` which is line 1  :)
        Line::new(line.saturating_sub(start) + 1).unwrap()
    }
}

#[cfg(test)]
mod test {

    use crate::{node::node_in, path::Path, read_doc, snippet::Line};

    #[test]
    fn strange_case() {
        let secondary = indoc::indoc! {r#"
            ---
            person:
              name: Steve E. Anderson
              age: 12
            "#};
        let secondary = read_doc(secondary, camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        assert_eq!(secondary.start, 2);
        assert_eq!(secondary.first_line, Line::unchecked(1));

        // the line after `age: 12` counts as there is a newline after the 2!
        assert_eq!(secondary.end, 5);
        assert_eq!(secondary.last_line, Line::unchecked(3));
    }

    #[test]
    fn relave_line_numbers() {
        let content = indoc::indoc! {r#"
        ---
        person:
          name: Steve E. Anderson
          age: 12
        ---
        pet:
          kind: cat
          age: 7
          breed: American Shorthair
        "#};

        let mut yaml = read_doc(content, camino::Utf8PathBuf::new()).unwrap();

        let first = yaml.remove(0);
        let second = yaml.remove(0);

        // Let's check that we are on the same page...
        // ...the first line of the first document comes after the `---`
        assert_eq!(first.start, 2);
        assert_eq!(first.first_line, Line::unchecked(1));
        // ...same for the first line of the second document.
        // we just keep counting
        assert_eq!(second.first_line, Line::unchecked(1));

        // the last line is the first line where indentation "resets"
        // this makes the range [first_line, last_line)
        assert_eq!(first.last_line, Line::unchecked(3));
        assert_eq!(second.last_line, Line::unchecked(4));

        // .person starts on line 2 according to the debug output
        assert_eq!(first.relative_line(2), Line::unchecked(1));

        // .pet starts on 6
        assert_eq!(second.relative_line(6), Line::unchecked(1));
    }

    #[test]
    fn real_life_relative_numbers() {
        let with_line_numbers = indoc::indoc! {
            r#"( 0) ---
               ( 1) apiVersion: v1
               ( 2) kind: Service
               ( 3) metadata:
               ( 4)   name: flux-engine-steam
               ( 5)   namespace: classification
               ( 6)   labels:
               ( 7)     helm.sh/chart: flux-engine-steam-2.28.12
               ( 8)     app.kubernetes.io/name: flux-engine-steam
               ( 9)     app: flux-engine-steam
               (10)     app.kubernetes.io/version: 0.0.27-pre1
               (11)     app.kubernetes.io/managed-by: batman
               (12)   annotations:
               (13)     github.com/repository_url: git@github.com:flux-engine-steam
               (14)     this_is: new
               (15) spec:
               (16)   ports:
               (17)     - targetPort: 8502
               (18)       port: 3000
               (19)       name: https
               (20)   selector:
               (21)     app: flux-engine-steam
               (22) ---
               (23) foo: bar
               (24) name: Bob"#,
        };

        let content = with_line_numbers
            .lines()
            .map(|line| line.chars().skip("( 0) ".len()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        let mut sources = read_doc(content, camino::Utf8PathBuf::new()).unwrap();

        let first = sources.remove(0);
        let spec = node_in(&first.yaml, &Path::parse_str(".spec")).unwrap();

        assert_eq!(spec.span.start.line(), 17);

        assert_eq!(first.start, 2);
        assert_eq!(first.end, 23);
        assert_eq!(first.first_line, Line::unchecked(1));
        assert_eq!(first.last_line, Line::unchecked(21));

        // NOTE: No idea if this is right,
        assert_eq!(first.relative_line(14), Line::unchecked(13));

        let second = sources.remove(0);

        assert_eq!(second.start, 24);
        assert_eq!(second.end, 26);
        assert_eq!(second.first_line, Line::unchecked(1));
        assert_eq!(second.last_line, Line::unchecked(2));
    }
}
