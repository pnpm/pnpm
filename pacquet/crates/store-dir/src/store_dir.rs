use dashmap::DashSet;
use serde::{Deserialize, Serialize};
use sha2::{Sha512, digest};
use std::{
    path::{self, PathBuf},
    sync::OnceLock,
};

/// Content hash of a file.
pub type FileHash = digest::Output<Sha512>;

/// Major version of the pnpm store layout that pacquet writes to and reads
/// from. Mirrors pnpm's [`STORE_VERSION`](https://github.com/pnpm/pnpm/blob/29a42efc3b/core/constants/src/index.ts#L9).
///
/// The constant is part of the public contract pnpm exposes to every
/// project's `.modules.yaml` (the recorded `storeDir` is the
/// `STORE_VERSION`-suffixed path), so changing it requires moving in
/// lockstep with pnpm — otherwise both tools start refusing each
/// other's stores with `ERR_PNPM_UNEXPECTED_STORE`.
pub const STORE_VERSION: &str = "v11";

/// Represent a store directory.
///
/// * The store directory stores all files that were acquired by installing packages with pacquet or pnpm.
/// * The files in `node_modules` directories are hardlinks or reflinks to the files in the store directory.
/// * The store directory can and often act as a global shared cache of all installation of different workspaces.
/// * The location of the store directory can be customized by `store-dir` field.
/// * The on-disk layout matches pnpm v11 (`<root>/files/XX/…[-exec]` + `<root>/index.db`)
///   where `<root>` already includes the `v11` suffix, so the two tools share both the
///   physical layout *and* the user-visible `storeDir` string written to
///   `.modules.yaml`.
//
// `#[serde(from = "PathBuf", into = "PathBuf")]` routes both
// directions through the `PathBuf` boundary so deserialization goes
// back through [`From<PathBuf>`] and the [`STORE_VERSION`] suffix
// invariant holds for persisted unsuffixed paths too — the previous
// `#[serde(transparent)]` derive deserialised straight into the
// `root` field and bypassed the auto-append.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "PathBuf", into = "PathBuf")]
pub struct StoreDir {
    /// The `STORE_VERSION`-suffixed store path, equivalent to pnpm's
    /// [`storeDir`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42).
    /// Consumers should reach for the purpose-built helpers
    /// ([`Self::files`][], [`Self::tmp`], [`Self::links`],
    /// [`Self::projects`]) rather than this raw path.
    root: PathBuf,

    /// Runtime cache of shard bytes (`files/XX/`) this process has already
    /// ensured exist. The CAS layout has exactly 256 shards keyed by the
    /// first byte of the sha512 digest; `create_dir_all` is idempotent but
    /// does a `stat` syscall every call even when the directory already
    /// exists, and a cold install of ~10k files would otherwise pay that
    /// `stat` per file. After the first hit, the shard is cached and
    /// subsequent writes skip the syscall entirely. Populated lazily by
    /// [`StoreDir::write_cas_file`]; duplicate inserts across threads are
    /// harmless since `create_dir_all` is idempotent. Not part of the
    /// serialised wire shape — `#[serde(from/into)]` round-trips only
    /// through `PathBuf`, so the cache is regenerated empty on every
    /// deserialise.
    ensured_shards: DashSet<u8>,

    /// Memoised `<root>/files` directory. Resolved lazily on the first
    /// CAS path lookup and reused across every subsequent file write
    /// — saves one `Path::join` allocation per file on the hot path,
    /// ~170k on the alotta-files clean install. `OnceLock` so
    /// initialization across rayon threads stays race-free.
    #[serde(skip, default)]
    cached_files_dir: OnceLock<PathBuf>,
}

impl From<StoreDir> for PathBuf {
    fn from(store_dir: StoreDir) -> Self {
        store_dir.root
    }
}

/// Manual `PartialEq` / `Eq`: the shard cache is runtime state, two stores
/// are equal iff they point at the same path.
impl PartialEq for StoreDir {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl Eq for StoreDir {}

impl From<PathBuf> for StoreDir {
    /// Wrap a raw path into a [`StoreDir`], appending [`STORE_VERSION`]
    /// when the path doesn't already end with that segment. Mirrors
    /// pnpm's [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42),
    /// so both tools record the same `storeDir` string in
    /// `.modules.yaml` and switching between them stops tripping
    /// `ERR_PNPM_UNEXPECTED_STORE`.
    fn from(root: PathBuf) -> Self {
        let root = if root.file_name().and_then(|name| name.to_str()) == Some(STORE_VERSION) {
            root
        } else {
            root.join(STORE_VERSION)
        };
        StoreDir { root, ensured_shards: DashSet::new(), cached_files_dir: OnceLock::new() }
    }
}

impl StoreDir {
    /// Construct an instance of [`StoreDir`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        root.into().into()
    }

    /// Mark the shard keyed by the first byte of a sha512 digest as "parent
    /// directory already created this process". Used by
    /// [`StoreDir::write_cas_file`] to skip `create_dir_all` on subsequent
    /// writes into the same shard.
    pub(crate) fn mark_shard_ensured(&self, shard_byte: u8) {
        self.ensured_shards.insert(shard_byte);
    }

    /// Fast-path check: did this process already ensure the shard dir for
    /// this byte exists? Returns `true` once, per shard, per process.
    pub(crate) fn shard_already_ensured(&self, shard_byte: u8) -> bool {
        self.ensured_shards.contains(&shard_byte)
    }

    /// Create an object that [displays](std::fmt::Display) the root of the store directory.
    pub fn display(&self) -> path::Display<'_> {
        self.root.display()
    }

    /// The directory that contains all content-addressed files.
    fn files(&self) -> PathBuf {
        self.files_dir().clone()
    }

    /// Borrow the memoised `<root>/files` path. The CAS write hot
    /// path calls this per CAFS file written, so caching the joined
    /// path saves one `PathBuf` allocation per call (~170k on the
    /// alotta-files clean install).
    fn files_dir(&self) -> &PathBuf {
        self.cached_files_dir.get_or_init(|| self.root.join("files"))
    }

    /// Path to a file in the store directory.
    ///
    /// **Parameters:**
    /// * `head` is the first 2 hexadecimal digit of the file address.
    /// * `tail` is the rest of the address and an optional suffix.
    fn file_path_by_head_tail(&self, head: &str, tail: &str) -> PathBuf {
        self.files_dir().join(head).join(tail)
    }

    /// Path to a content-addressed file. The hex digest is split into a
    /// two-char prefix directory and the remainder, plus an optional `-exec`
    /// suffix for executable files — this is pnpm v11's `files/XX/<rest>[-exec]`
    /// layout.
    pub(crate) fn file_path_by_hex_str(&self, hex: &str, suffix: &'static str) -> PathBuf {
        let head = &hex[..2];
        let middle = &hex[2..];
        let tail = format!("{middle}{suffix}");
        self.file_path_by_head_tail(head, &tail)
    }

    /// Path to the temporary directory inside the store.
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Path to the shared global-virtual-store directory inside the
    /// store. Matches pnpm's
    /// [`extendInstallOptions.ts:350-358`](https://github.com/pnpm/pnpm/blob/29a42efc3b/installing/deps-installer/src/install/extendInstallOptions.ts#L350-L358):
    /// `globalVirtualStoreDir = path.join(extendedOpts.storeDir, 'links')`.
    /// `extendedOpts.storeDir` has already been routed through
    /// [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42)
    /// — which appends [`STORE_VERSION`] (`"v11"`) to whatever the
    /// user configured — by the time that join runs. Pacquet's
    /// [`StoreDir::from`] applies the same suffix, so `self.root` is
    /// already the v11 path and the on-disk location is
    /// `<root>/links`, identical to pnpm's. Sharing this path across
    /// pnpm and pacquet is the whole point.
    pub fn links(&self) -> PathBuf {
        self.root.join("links")
    }

    /// Path to the per-store projects registry — a flat directory of
    /// symlinks (`<store>/projects/<short-hash>` → project dir) the
    /// global-virtual-store prune sweep walks when deciding which
    /// `<store>/links/...` slots are still referenced. Mirrors pnpm
    /// 11's
    /// [`{storeDir}/projects/` layout](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/controller/CHANGELOG.md#L136)
    /// — `<store>` already carries the v11 suffix on both sides per
    /// [`Self::links`].
    pub fn projects(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Borrow the raw store-root path. Most code should prefer the
    /// purpose-built helpers (`v11`, `tmp`, `links`, `projects`); this
    /// is for the few callers that need to compute a sibling path the
    /// helpers don't cover.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// On a fresh store, eagerly create `<store>/files/` plus every
    /// `files/XX/` shard (00..ff) and seed the shard cache with the
    /// bytes we just created, so CAFS writes never pay a
    /// `create_dir_all` syscall in the hot path.
    ///
    /// Gated by an `is_dir()` check on `files/` so we only run when the
    /// store is truly fresh — spiritually matches pnpm's
    /// [`createPackageStore`](https://github.com/pnpm/pnpm/blob/1819226b51/store/controller/src/storeController/index.ts)
    /// guard (`if !fs.existsSync(path.join(storeDir, 'files')) initStoreDir(...)`),
    /// but tightened from `exists()` to `is_dir()` so a non-directory
    /// entry at `files/` doesn't let `init` silently noop past store
    /// corruption. On a warm store this is a single stat and we
    /// return `Ok(())` without seeding the cache: a store created by
    /// an older pacquet that only lazily materialized shards might
    /// not have every `files/XX/` on disk, and pre-seeding the cache
    /// would let a later `write_cas_file` skip `ensure_parent_dir`
    /// and then fail at `open` with `NotFound`. Leaving the cache
    /// empty on warm store lets the lazy mkdir fallback inside
    /// [`StoreDir::write_cas_file`] populate it per shard on first
    /// write — the same shape pnpm uses via `writeFile.ts`'s `dirs`
    /// Set.
    ///
    /// Errors from individual shard mkdirs are ignored when the error is
    /// [`AlreadyExists`][std::io::ErrorKind::AlreadyExists] **and** the
    /// existing entry is actually a directory (via
    /// [`Path::is_dir`][std::path::Path::is_dir], which follows
    /// symlinks — a symlink pointing at a real directory
    /// is accepted, matching what ops folks sometimes do to spread a
    /// store across disks). This matches pnpm's try/catch per shard
    /// (parallel process racing the same layout is benign) but
    /// tightens it slightly: a regular file, a non-directory symlink,
    /// or a broken symlink squatting on the shard path is rejected
    /// instead of being cached as ensured. Other errors propagate; the
    /// caller degrades them to a warning and falls back to the per-
    /// write lazy mkdir.
    pub fn init(&self) -> std::io::Result<()> {
        let files = self.files();
        // `is_dir()` rather than `exists()`: if `files` is present but
        // isn't a directory (regular file, broken symlink, other
        // corruption), a permissive `exists()` check would make `init`
        // a silent noop and later `write_cas_file` calls would fail
        // with cryptic per-file `open` errors. Gating on `is_dir()`
        // lets the `create_dir_all` below surface a clear "not a
        // directory" error from the kernel, which the caller degrades
        // to a `warn!` at install bootstrap.
        if files.is_dir() {
            return Ok(());
        }
        std::fs::create_dir_all(&files)?;
        for shard in 0u8..=255 {
            // Two-char lowercase hex keyed off the first byte of the
            // sha512 digest, matching `StoreDir::file_path_by_hex_str`.
            let shard_dir = files.join(format!("{shard:02x}"));
            if let Err(error) = std::fs::create_dir(&shard_dir) {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
                // `AlreadyExists` is benign only when the existing
                // entry resolves to a directory — a parallel pnpm
                // or pacquet process racing the same layout is
                // fine, and a symlink pointing at a real directory
                // is too (ops folks occasionally spread a store
                // across disks that way). `Path::is_dir` follows
                // symlinks, which is the desired semantics here. A
                // regular file, a non-dir symlink, or a broken
                // symlink would make `mark_shard_ensured` a lie and
                // punt the failure to a much less actionable
                // `open` error inside the per-file CAFS write.
                // Reject upfront.
                if !shard_dir.is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!(
                            "CAFS shard path {} exists but does not resolve to a directory",
                            shard_dir.display(),
                        ),
                    ));
                }
            }
            self.mark_shard_ensured(shard);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
