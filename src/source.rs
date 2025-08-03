use std::cmp::max;

use crate::snippet::Line;

// TODO: Should this live elsewhere?
#[derive(Debug, Clone)]
pub struct YamlSource {
    pub file: camino::Utf8PathBuf,
    pub yaml: saphyr::MarkedYamlOwned,
    pub content: String,
    pub index: usize,
    // these numbers are based on the file itself.
    // they do come from the parser, but carry on counting
    // up across multiple docs within the same file
    pub start: usize,
    pub end: usize,
    // these are relative numbers.
    // Unless something is funky, first line should always be Line(1)
    pub first_line: Line,
    pub last_line: Line,
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
        log::info!(
            "the start of the document is on absolute line {}, and we are checking for line {line}",
            self.start
        );
        let raw = max(1, line.saturating_sub(self.start));

        Line::new(raw).unwrap()
    }
}

#[cfg(test)]
mod test {
    use crate::{node::node_in, path::Path, read_doc, snippet::Line};

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

        dbg!(&spec);
        assert_eq!(spec.span.start.line(), 17); // (???)

        assert_eq!(first.start, 2);
        assert_eq!(first.end, 23);
        assert_eq!(first.first_line, Line::unchecked(1));
        assert_eq!(first.last_line, Line::unchecked(21));

        // NOTE: No idea if this is right,
        assert_eq!(first.relative_line(15), Line::unchecked(13));

        let second = sources.remove(0);

        assert_eq!(second.start, 24);
        assert_eq!(second.end, 26);
        assert_eq!(second.first_line, Line::unchecked(1));
        assert_eq!(second.last_line, Line::unchecked(2));

        // assert_eq!(first.relative_line(15), Line::unchecked(14));
        panic!("???");
    }
}
