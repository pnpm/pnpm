use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// A snapshot of the repository's files: the working tree or a git revision.
/// Paths are relative to the repository root and use `/`.
pub trait Tree {
    /// The file's content, or `None` when the snapshot has no such file.
    fn read(&self, path: &str) -> Result<Option<String>, String>;

    /// The names of the directories directly inside `dir`.
    fn subdirectories(&self, dir: &str) -> Result<Vec<String>, String>;
}

pub struct WorkingTree {
    pub root: PathBuf,
}

impl Tree for WorkingTree {
    fn read(&self, path: &str) -> Result<Option<String>, String> {
        let full_path = self.root.join(path);
        match fs::read_to_string(&full_path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("read {full_path:?}: {error}")),
        }
    }

    fn subdirectories(&self, dir: &str) -> Result<Vec<String>, String> {
        let full_path = self.root.join(dir);
        let entries =
            fs::read_dir(&full_path).map_err(|error| format!("list {full_path:?}: {error}"))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| format!("list {full_path:?}: {error}"))?;
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                names.push(
                    entry
                        .file_name()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        Ok(names)
    }
}

/// The files of one commit, read through `git` so the working tree is left alone.
pub struct GitRevision {
    root: PathBuf,
    commit: String,
    files: BTreeSet<String>,
}

impl GitRevision {
    /// Resolves `revision` to a commit id first, so the later `git`
    /// arguments carry a hex id and never something `git` reads as an option.
    pub fn open(root: &Path, revision: &str) -> Result<Self, String> {
        let spec = format!("{revision}^{{commit}}");
        let resolved = git(root, &["rev-parse", "--verify", "--end-of-options", &spec])?;
        let commit = resolved.trim().to_owned();
        let listing = git(root, &["ls-tree", "-r", "--name-only", &commit])?;
        let files = listing
            .lines()
            .map(str::to_owned)
            .collect();
        Ok(Self { root: root.to_path_buf(), commit, files })
    }
}

impl Tree for GitRevision {
    fn read(&self, path: &str) -> Result<Option<String>, String> {
        if !self.files.contains(path) {
            return Ok(None);
        }
        git(&self.root, &["show", &format!("{}:{path}", self.commit)]).map(Some)
    }

    fn subdirectories(&self, dir: &str) -> Result<Vec<String>, String> {
        let prefix = format!("{dir}/");
        let names = self.files
            .iter()
            .filter_map(|file| file.strip_prefix(&prefix)?.split_once('/'))
            .map(|(name, _)| name.to_owned())
            .collect::<BTreeSet<_>>();
        Ok(names.into_iter().collect())
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("run git {args:?}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {args:?} failed: {}", stderr.trim()));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("git {args:?} printed non-UTF-8: {error}"))
}
