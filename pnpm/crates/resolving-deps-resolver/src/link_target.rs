//! Re-anchor workspace `link:` targets in lockfile-root-relative space.
//!
//! A `link:` target is stored relative to one anchor (the lockfile root
//! or the consuming importer's directory) and repeatedly re-expressed
//! against the other, once per dependency edge. Doing that math on the
//! absolute paths walks and re-allocates the anchors' long common
//! prefix on every edge; both anchors hang off the same lockfile root,
//! so the prefix always cancels out. The helpers here run the same
//! component math on the root-relative suffixes instead — and since
//! both anchors are constant per importer, [`ImporterAnchor`] derives
//! the suffix once so the per-edge work is the suffix math alone.
//!
//! The anchor is a *fast path*: it disarms itself for any input where
//! the relative-space result is not trivially the same as the
//! absolute-space one — a dot-carrying anchor, a driveless-yet-rooted
//! Windows anchor, an importer outside the lockfile root — and each
//! rendering falls back to the caller's absolute-space math the same
//! way for an absolute or escaping target. Within the guarded domain
//! the two computations agree component-for-component, because
//! `diff_paths` and `lexical_normalize` are lexical: prefixing both
//! arguments with the same clean root never changes the outcome.
//!
//! Each rendering returns the final `link:`-body string — the
//! forward-slashed form every caller previously produced from the
//! `PathBuf` via `display` + `replace` — so the per-edge cost is one
//! pass over the target's components and a single allocation.

use std::path::{Component, Path};

/// The importer-side inputs of `link:` re-anchoring, derived once per
/// importer: its directory as a clean relative suffix of the lockfile
/// root, pre-split into components. The workspace root importer holds
/// the empty suffix.
#[derive(Debug, Default, Clone)]
pub(crate) struct ImporterAnchor {
    /// `None` — an anchor carries dot components, is not truly
    /// absolute, is not valid UTF-8, or the importer is not under the
    /// lockfile root — turns every rendering into the caller's
    /// fallback.
    rel_components: Option<Vec<String>>,
}

impl ImporterAnchor {
    pub(crate) fn new(project_dir: &Path, lockfile_dir: &Path) -> Self {
        ImporterAnchor {
            rel_components: importer_rel_dir(project_dir, lockfile_dir).and_then(|rel| {
                rel.components()
                    .map(|component| component.as_os_str().to_str().map(str::to_owned))
                    .collect()
            }),
        }
    }

    /// Express a lockfile-root-relative `target` relative to the
    /// importer, as `diff_paths` against the importer suffix would:
    /// climb out of the suffix past the shared prefix, then descend
    /// into the target. `None` for a disarmed anchor, an anchored
    /// target, or one that carries `..` components.
    pub(crate) fn target_relative_to_importer(&self, target: &str) -> Option<String> {
        let rel = self.rel_components.as_deref()?;
        if target.starts_with(SEPARATORS) {
            return None;
        }
        // Avoid splitting the target a second time for the rendering:
        // when the kept tail is verbatim — nothing collapsed or
        // dropped — it copies over as one slice.
        let mut shared = 0;
        let mut still_shared = true;
        let mut tail_start: Option<usize> = None;
        let mut tail_verbatim = true;
        let mut pos = 0;
        loop {
            let end = target[pos..].find(SEPARATORS).map_or(target.len(), |offset| pos + offset);
            let segment = &target[pos..end];
            if segment.is_empty() || segment == "." {
                if tail_start.is_some() {
                    tail_verbatim = false;
                }
            } else if segment == ".." {
                return None;
            } else {
                #[cfg(windows)]
                if pos == 0 && segment.contains(':') {
                    return None;
                }
                if still_shared && rel.get(shared).is_some_and(|name| name == segment) {
                    shared += 1;
                } else {
                    still_shared = false;
                    if tail_start.is_none() {
                        tail_start = Some(pos);
                    }
                }
            }
            if end == target.len() {
                break;
            }
            pos = end + 1;
        }
        let climb = rel.len() - shared;
        let tail = match tail_start {
            Some(tail_start) if tail_verbatim => &target[tail_start..],
            None => "",
            // A tail that dropped or collapsed segments re-renders
            // through the general segment join.
            Some(_) => {
                return Some(render(
                    std::iter::repeat_n("..", climb).chain(relative_segments(target)?.skip(shared)),
                    3 * climb + target.len(),
                ));
            }
        };
        let mut rendered = String::with_capacity(3 * climb + tail.len());
        for _ in 0..climb {
            if !rendered.is_empty() {
                rendered.push('/');
            }
            rendered.push_str("..");
        }
        if !tail.is_empty() {
            if !rendered.is_empty() {
                rendered.push('/');
            }
            rendered.push_str(tail);
        }
        if rendered.contains('\\') {
            rendered = rendered.replace('\\', "/");
        }
        Some(rendered)
    }

    /// Express an importer-relative `target` (which typically climbs
    /// out of the importer via `..` components) relative to the
    /// lockfile root, as `lexical_normalize` over the joined suffix
    /// would: `..` pops the nearest kept component. `None` for a
    /// disarmed anchor, an anchored target, or one that escapes the
    /// lockfile root. The suffix components are all normal, so the
    /// first climb past them pins a leading `..` that nothing later
    /// can remove.
    pub(crate) fn target_relative_to_lockfile_root(&self, target: &str) -> Option<String> {
        let rel = self.rel_components.as_deref()?;
        let mut kept = rel.len();
        let mut descended: Vec<&str> = Vec::new();
        for segment in relative_segments(target)? {
            if segment == ".." {
                if descended.pop().is_none() {
                    kept = kept.checked_sub(1)?;
                }
            } else {
                descended.push(segment);
            }
        }
        Some(render(
            rel[..kept].iter().map(String::as_str).chain(descended),
            rel[..kept].iter().map(|name| name.len() + 1).sum::<usize>() + target.len(),
        ))
    }
}

#[cfg(windows)]
const SEPARATORS: &[char] = &['/', '\\'];
#[cfg(not(windows))]
const SEPARATORS: &[char] = &['/'];

/// Split a relative `target` into the segments `Path::components`
/// would yield, without the per-component `OsStr` machinery:
/// separators collapse and `.` segments vanish (`lexical_normalize`
/// erases a leading one too, so the two directions agree on it).
/// `None` for an anchored target — one starting with a separator, or
/// (on Windows) whose first segment carries a `:` and so may be a
/// prefix — where suffix math does not apply; the caller's
/// absolute-space fallback handles those.
fn relative_segments(target: &str) -> Option<impl Iterator<Item = &str> + Clone> {
    if target.starts_with(SEPARATORS) {
        return None;
    }
    #[cfg(windows)]
    if target.split(SEPARATORS).next().is_some_and(|first| first.contains(':')) {
        return None;
    }
    Some(target.split(SEPARATORS).filter(|segment| !segment.is_empty() && *segment != "."))
}

/// Join components with `/` and normalize any backslash inside one, the
/// way the callers' former `display` + `replace('\\', "/")` pass did.
fn render<'component>(
    components: impl Iterator<Item = &'component str>,
    capacity: usize,
) -> String {
    let mut rendered = String::with_capacity(capacity);
    for component in components {
        if !rendered.is_empty() {
            rendered.push('/');
        }
        rendered.push_str(component);
    }
    if rendered.contains('\\') {
        rendered = rendered.replace('\\', "/");
    }
    rendered
}

/// The importer's directory as a clean relative suffix of the lockfile
/// root. `None` — an anchor carries `.` / `..` components, or the
/// importer is not under the root — sends the caller to the
/// absolute-space math. The workspace root importer yields the empty
/// path.
pub(crate) fn importer_rel_dir<'dir>(
    project_dir: &'dir Path,
    lockfile_dir: &Path,
) -> Option<&'dir Path> {
    if !is_clean_absolute(lockfile_dir) {
        return None;
    }
    let rel = project_dir.strip_prefix(lockfile_dir).ok()?;
    all_normal(rel).then_some(rel)
}

fn is_clean_absolute(path: &Path) -> bool {
    // `is_absolute` carries the platform rules — on Windows it demands
    // a prefix *and* a root, rejecting drive-relative `C:foo` and
    // rootless `\foo` alike.
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn all_normal(path: &Path) -> bool {
    path.components().all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests;
