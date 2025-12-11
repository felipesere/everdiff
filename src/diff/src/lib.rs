use std::io::Read;

pub mod diff;
pub mod line;
pub mod node;
pub mod path;
pub mod source;

pub use diff::{ArrayOrdering, Context, Difference, Item, diff};
pub use line::Line;
pub use path::{IgnorePath, Path, Segment};
pub use source::{YamlSource, read_doc};

// TODO: Optimize memory usage for large files - consider streaming approach instead of loading all into memory
pub fn read(paths: &[camino::Utf8PathBuf]) -> anyhow::Result<Vec<YamlSource>> {
    let mut docs = Vec::new();
    for p in paths {
        let mut f = std::fs::File::open(p)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;

        let n = read_doc(content, p.clone())?;

        docs.extend(n.into_iter());
    }

    Ok(docs)
}
