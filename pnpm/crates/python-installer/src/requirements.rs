use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_python_resolver::parse_requirement;
use std::{
    collections::BTreeSet,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 1024;
const MAX_DEPTH: usize = 64;
const MAX_REQUIREMENTS: usize = 100_000;

pub(super) fn read(path: &Path, root: &Path) -> Result<Vec<String>> {
    let root = fs::canonicalize(root).into_diagnostic()?;
    let mut parser = Parser {
        root,
        visiting: BTreeSet::new(),
        completed: BTreeSet::new(),
        bytes: 0,
        requirements: BTreeSet::new(),
    };
    parser.read_file(path)?;
    Ok(parser.requirements.into_iter().collect())
}

struct Parser {
    root: PathBuf,
    visiting: BTreeSet<PathBuf>,
    completed: BTreeSet<PathBuf>,
    bytes: usize,
    requirements: BTreeSet<String>,
}

impl Parser {
    fn read_file(&mut self, path: &Path) -> Result<()> {
        let canonical = fs::canonicalize(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", path.display()))?;
        if !canonical.starts_with(&self.root) {
            bail!("Python requirements file escapes {}: {}", self.root.display(), path.display());
        }
        if self.visiting.contains(&canonical) {
            bail!("cyclic Python requirements include: {}", path.display());
        }
        if self.completed.contains(&canonical) {
            return Ok(());
        }
        if self.visiting.len() >= MAX_DEPTH
            || self.completed.len() + self.visiting.len() >= MAX_FILES
        {
            bail!("Python requirements include limit exceeded at {}", path.display());
        }
        self.visiting.insert(canonical.clone());
        let contents = read_regular_file(&canonical)?;
        self.bytes += contents.len();
        if self.bytes > MAX_TOTAL_BYTES {
            bail!("Python requirements size limit exceeded at {}", path.display());
        }
        self.read_entries(&canonical, &contents)?;
        self.visiting.remove(&canonical);
        self.completed.insert(canonical);
        Ok(())
    }

    fn read_entries(&mut self, path: &Path, contents: &str) -> Result<()> {
        let mut logical = String::new();
        for (index, line) in contents.lines().enumerate() {
            if let Some(line) = line.strip_suffix('\\') {
                logical.push_str(line);
                continue;
            }
            logical.push_str(line);
            let entry = strip_comment(&logical);
            self.read_entry(path, index + 1, entry)?;
            logical.clear();
        }
        if !logical.is_empty() {
            bail!("unfinished Python requirement continuation in {}", path.display());
        }
        Ok(())
    }

    fn read_entry(&mut self, path: &Path, line: usize, entry: &str) -> Result<()> {
        if entry.is_empty() {
            return Ok(());
        }
        let context = || format!("{}:{line}", path.display());
        if let Some(include) = entry
            .strip_prefix("-r")
            .or_else(|| entry.strip_prefix("--requirement "))
            .or_else(|| entry.strip_prefix("--requirement="))
        {
            let include = include.trim();
            if include.is_empty() || include.contains("://") || Path::new(include).is_absolute() {
                bail!("unsupported Python requirements include at {}: {entry}", context());
            }
            let included = path
                .parent()
                .expect("requirements file has a parent")
                .join(include);
            self.read_file(&included).wrap_err_with(context)?;
        } else {
            if entry.starts_with('-') {
                bail!("unsupported Python requirements directive at {}: {entry}", context());
            }
            let requirement = parse_requirement(entry).wrap_err_with(context)?;
            self.requirements.insert(requirement.to_string());
            if self.requirements.len() > MAX_REQUIREMENTS {
                bail!("Python requirement count limit exceeded at {}", context());
            }
        }
        Ok(())
    }
}

fn read_regular_file(path: &Path) -> Result<String> {
    let context = || format!("read Python requirements file {}", path.display());
    if !fs::metadata(path)
        .into_diagnostic()
        .wrap_err_with(context)?
        .is_file()
    {
        bail!("Python requirements must be a regular file: {}", path.display());
    }
    let file = fs::File::open(path).into_diagnostic().wrap_err_with(context)?;
    if !file
        .metadata()
        .into_diagnostic()?
        .is_file()
    {
        bail!("Python requirements must be a regular file: {}", path.display());
    }
    let mut contents = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut contents)
        .into_diagnostic()
        .wrap_err_with(context)?;
    if contents.len() as u64 > MAX_FILE_BYTES {
        bail!("Python requirements file exceeds {MAX_FILE_BYTES} bytes: {}", path.display());
    }
    Ok(contents)
}

fn strip_comment(line: &str) -> &str {
    let entry = line.trim();
    entry
        .char_indices()
        .find(|(index, character)| {
            *character == '#' && (*index == 0 || entry[..*index].ends_with(char::is_whitespace))
        })
        .map_or(entry, |(index, _)| &entry[..index])
        .trim()
}

#[cfg(test)]
mod tests;
