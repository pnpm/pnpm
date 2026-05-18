use std::{
    path::Path,
    process::{Command, Stdio},
};
use which::which;

pub async fn ensure_virtual_registry(registry: &str) {
    if let Err(error) = reqwest::Client::new().head(registry).send().await {
        eprintln!("HEAD request to {registry} returned an error");
        eprintln!("Make sure the registry server is operational");
        panic!("{error}");
    };
}

pub fn ensure_git_repo(path: &Path) {
    assert!(path.is_dir());
    assert!(path.join(".git").is_dir());
    assert!(path.join("Cargo.toml").is_file());
    assert!(path.join("Cargo.lock").is_file());
}

pub fn ensure_program(program: &str) -> Command {
    match which(program) {
        Ok(real_program) => Command::new(real_program),
        Err(which::Error::CannotFindBinaryPath) => panic!("Cannot find {program} in $PATH"),
        Err(error) => panic!("{error}"),
    }
}

pub fn validate_revision_list<List>(list: List)
where
    List: IntoIterator,
    List::Item: AsRef<str>,
{
    for revision in list {
        let revision = revision.as_ref();
        if revision.starts_with('.') {
            eprintln!("Revision {revision:?} is invalid");
            panic!("Revision cannot start with a dot");
        }
        let invalid_char = revision.chars().find(|char| !matches!(char, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '+' | '.' | '~' | '^'));
        if let Some(char) = invalid_char {
            eprintln!("Revision {revision:?} is invalid");
            panic!("Invalid character: {char:?}");
        }
    }
}

pub fn executor<'a>(message: &'a str) -> impl FnOnce(&'a mut Command) {
    move |command| {
        let output = command
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .expect(message);
        assert!(output.status.success(), "Process exits with non-zero status: {message}");
    }
}
