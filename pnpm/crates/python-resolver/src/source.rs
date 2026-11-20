use crate::{
    LockedVcs,
    validate_url,
};
use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use url::Url;

/// The explicit source of a PEP 508 requirement.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Wheel { url: Url, sha256: Option<String> },
    Git(LockedVcs),
}

impl Source {
    #[must_use]
    pub fn compatible_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Wheel { url: left, sha256: a }, Self::Wheel { url: right, sha256: b }) => {
                left == right && (a.is_none() || b.is_none() || a == b)
            }
            _ => self == other,
        }
    }

    pub fn parse(source: &str) -> Result<Self> {
        if let Some(repository) = source.strip_prefix("git+") {
            return parse_git(repository).map(Self::Git);
        }
        let mut url: Url = source.parse().into_diagnostic()?;
        validate_url(&url)?;
        let sha256 = fragment(&url, "sha256")?.map(|digest| digest.to_ascii_lowercase());
        if sha256
            .as_ref()
            .is_some_and(|digest| {
                digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            bail!("invalid SHA-256 digest in Python source URL");
        }
        url.set_fragment(None);
        Ok(Self::Wheel { url, sha256 })
    }
}

fn fragment(url: &Url, key: &str) -> Result<Option<String>> {
    let Some(fragment) = url.fragment() else { return Ok(None) };
    let pairs = url::form_urlencoded::parse(fragment.as_bytes()).collect::<Vec<_>>();
    let [(name, value)] = pairs.as_slice() else {
        bail!("Python source URLs support only a {key} fragment");
    };
    if name != key {
        bail!("Python source URLs support only a {key} fragment");
    }
    Ok(Some(value.to_string()))
}

fn parse_git(repository: &str) -> Result<LockedVcs> {
    let mut url: Url = repository.parse().into_diagnostic()?;
    if !matches!(url.scheme(), "https" | "ssh" | "file")
        || url.password().is_some()
        || (url.scheme() != "ssh" && !url.username().is_empty())
    {
        bail!("Python git sources require HTTPS, SSH or file URLs without embedded credentials");
    }
    let subdirectory = fragment(&url, "subdirectory")?;
    validate_subdirectory(subdirectory.as_deref())?;
    url.set_fragment(None);
    let path = url.path().to_string();
    let (path, revision) = path
        .rsplit_once('@')
        .unwrap_or((&path, "HEAD"));
    let revision = percent_encoding::percent_decode_str(revision)
        .decode_utf8()
        .into_diagnostic()?
        .into_owned();
    if revision.is_empty() || revision.starts_with('-') {
        bail!("invalid Python git revision {revision:?}");
    }
    url.set_path(path);
    let commit_id = if revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        revision.to_ascii_lowercase()
    } else {
        String::new()
    };
    Ok(LockedVcs {
        kind: "git".to_string(),
        url: url.to_string(),
        requested_revision: revision,
        commit_id,
        subdirectory,
    })
}

pub(crate) fn validate_subdirectory(subdirectory: Option<&str>) -> Result<()> {
    if subdirectory.is_some_and(|path| {
        std::path::Path::new(path)
            .components()
            .any(|component| {
                !matches!(component, std::path::Component::Normal(_) | std::path::Component::CurDir)
            })
    }) {
        bail!("Python git subdirectory must be relative and stay inside its repository");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
