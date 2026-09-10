use super::{
    Arc, DerivedPackuments, File, MAX_FRAGMENT_LEN, MAX_HEADERS_LEN, MAX_INDEX_LEN, MetaHeaders,
    MirrorFile, MirrorIndex, Package, PackageVersions, Path, Read, fs, parse_mirror_magic,
    raise_open_file_limit_once,
};

pub(super) fn load_meta_with_hold_cap(pkg_mirror: &Path, hold_cap: usize) -> Option<Package> {
    raise_open_file_limit_once();
    let mut file = File::open(pkg_mirror).ok()?;
    // The magic line plus the two length fields fit well inside this.
    let mut prefix = [0u8; 256];
    let filled = read_prefix(&mut file, &mut prefix)?;
    let Some(layout) = mirror_layout(&prefix[..filled])? else {
        return load_legacy_ndjson_meta(pkg_mirror);
    };
    let (headers, index, file_size) = read_mirror_records(&mut file, &prefix[..filled], &layout)?;
    let spans = absolute_spans(index.versions, layout.fragment_base, file_size)?;
    let versions = match MirrorFile::try_hold(file, hold_cap) {
        Ok(held) => PackageVersions::from_file_spans(&held, spans),
        Err(file) => buffer_fragments(&file, spans)?,
    };

    let mut meta = Package {
        name: index.name,
        dist_tags: index.dist_tags,
        versions,
        time: index.time,
        modified: headers.modified,
        etag: headers.etag,
        homepage: index.homepage,
        mutex: Arc::default(),
        derived: DerivedPackuments::default(),
    };
    meta.drop_incomplete_publish_times();
    Some(meta)
}

/// Where the headers and index records sit in a mirror file, per its
/// magic line.
pub(super) struct MirrorLayout {
    pub(super) headers_start: usize,
    pub(super) index_start: usize,
    pub(super) fragment_base: usize,
}

/// `None` for an unreadable prefix, `Some(None)` for the legacy NDJSON
/// format. Each declared length is bounded before the layout is
/// trusted.
pub(super) fn mirror_layout(prefix: &[u8]) -> Option<Option<MirrorLayout>> {
    let newline = prefix.iter().position(|&byte| byte == b'\n')?;
    let line = std::str::from_utf8(&prefix[..newline]).ok()?;
    let Some((headers_len, index_len)) = parse_mirror_magic(line) else {
        return Some(None);
    };
    if headers_len > MAX_HEADERS_LEN || index_len > MAX_INDEX_LEN {
        return None;
    }
    let headers_start = newline + 1;
    let index_start = headers_start.checked_add(headers_len)?;
    Some(Some(MirrorLayout {
        headers_start,
        index_start,
        fragment_base: index_start.checked_add(index_len)?,
    }))
}

/// Read the rest of the headers + index records, requiring the whole
/// region to fit inside the actual file before allocating a buffer for
/// it; the file's fragment section is only buffered on the held-handle
/// fallback.
pub(super) fn read_mirror_records(
    file: &mut File,
    prefix: &[u8],
    layout: &MirrorLayout,
) -> Option<(MetaHeaders, MirrorIndex, u64)> {
    let file_size = file.metadata().ok()?.len();
    if u64::try_from(layout.fragment_base).ok()? > file_size {
        return None;
    }
    let mut records =
        vec![0u8; layout.fragment_base.checked_sub(prefix.len().min(layout.fragment_base))?];
    file.read_exact(&mut records).ok()?;
    let mut prefixed = Vec::with_capacity(layout.fragment_base);
    prefixed.extend_from_slice(&prefix[..prefix.len().min(layout.fragment_base)]);
    prefixed.extend_from_slice(&records);
    let headers: MetaHeaders =
        serde_json::from_slice(prefixed.get(layout.headers_start..layout.index_start)?).ok()?;
    let index: MirrorIndex =
        serde_json::from_slice(prefixed.get(layout.index_start..layout.fragment_base)?).ok()?;
    Some((headers, index, file_size))
}

/// Rebase the relative spans and reject any that fall outside the file
/// — a truncated or hand-edited mirror reads as a miss rather than
/// handing out garbage fragments later.
pub(super) fn absolute_spans(
    versions: Vec<(String, u64, u32)>,
    fragment_base: usize,
    file_size: u64,
) -> Option<Vec<(String, u64, u32)>> {
    let mut spans = Vec::with_capacity(versions.len());
    for (version, offset, len) in versions {
        // A span past the fragment bound reads as an absent version
        // (the same contract as an undecodable fragment) rather than
        // rejecting the whole document: the bound exists to stop a
        // corrupt index from driving huge hydration allocations, and
        // the writer never persists such fragments.
        if len > MAX_FRAGMENT_LEN {
            continue;
        }
        let absolute = (fragment_base as u64).checked_add(offset)?;
        if absolute.checked_add(u64::from(len))? > file_size {
            return None;
        }
        spans.push((version, absolute, len));
    }
    Some(spans)
}

/// Held-handle budget exhausted (an unusually low descriptor limit, or an
/// install consulting more packuments than the cap): buffer this mirror's
/// fragments and close the file, so a full cache can never make `File::open`
/// fail elsewhere and turn present mirrors into cache misses.
///
/// Each validated span is read with its own positioned read: reading the
/// contiguous fragment region would let a corrupt index's sparse gaps
/// inflate the buffer far past the real fragment bytes. The budget bounds
/// the total even against an index whose spans overlap or repeat.
pub(super) fn buffer_fragments(
    file: &File,
    spans: Vec<(String, u64, u32)>,
) -> Option<PackageVersions> {
    const MAX_EAGER_FRAGMENT_TOTAL: u64 = 1 << 30;
    let mut budget = MAX_EAGER_FRAGMENT_TOTAL;
    let mut raw_fragments = Vec::with_capacity(spans.len());
    for (version, absolute, len) in spans {
        budget = budget.checked_sub(u64::from(len))?;
        let mut bytes = vec![0u8; len as usize];
        if pnpm_registry::read_exact_at(file, &mut bytes, absolute).is_err() {
            continue;
        }
        let Ok(json) = String::from_utf8(bytes) else { continue };
        let Ok(raw) = serde_json::from_str::<Box<serde_json::value::RawValue>>(&json) else {
            continue;
        };
        raw_fragments.push((version, raw));
    }
    Some(PackageVersions::from_raw_fragments(raw_fragments))
}

/// A legacy NDJSON mirror: the whole body after the header line is the
/// packument.
pub(super) fn load_legacy_ndjson_meta(pkg_mirror: &Path) -> Option<Package> {
    let contents = fs::read(pkg_mirror).ok()?;
    let newline = contents.iter().position(|&byte| byte == b'\n')?;
    let headers: MetaHeaders = serde_json::from_slice(&contents[..newline]).ok()?;
    let mut meta: Package = serde_json::from_slice(&contents[newline + 1..]).ok()?;
    meta.etag = headers.etag;
    meta.modified = meta.modified.or(headers.modified);
    meta.drop_incomplete_publish_times();
    Some(meta)
}

/// Fill `prefix` from the head of the file, returning how many bytes
/// arrived before EOF.
pub(super) fn read_prefix(file: &mut File, prefix: &mut [u8]) -> Option<usize> {
    let mut filled = 0usize;
    while filled < prefix.len() {
        let read = file.read(&mut prefix[filled..]).ok()?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Some(filled)
}

/// How many mirror files [`load_meta`](super::load_meta) may keep open at once. Sized
/// from the post-raise soft descriptor limit with headroom for the
/// rest of the install (sockets, tarball extraction, store writes);
/// loads beyond the cap buffer their fragments instead of holding a
/// handle.
pub(super) fn held_mirror_file_cap() -> usize {
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CAP.get_or_init(|| {
        raise_open_file_limit_once();
        soft_open_file_limit().map_or(1 << 19, |soft| (soft / 2).min(1 << 19))
    })
}

#[cfg(unix)]
pub(super) fn soft_open_file_limit() -> Option<usize> {
    let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: plain libc call; `limit` is a properly initialised
    // out-parameter and the pointer does not outlive the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 {
        return None;
    }
    usize::try_from(limit.rlim_cur).ok()
}

/// Windows has no `RLIMIT_NOFILE`; the per-process handle capacity is
/// far above the cap's upper clamp.
#[cfg(not(unix))]
pub(super) fn soft_open_file_limit() -> Option<usize> {
    None
}
