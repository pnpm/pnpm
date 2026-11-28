//! Archives whose headers node-tar, which npm reads packages with, could
//! read differently from this crate. The eager and the streaming path must
//! either read them as node-tar does or refuse them.

use super::{
    extract_gzipped_tarball,
    extract_tarball_entries,
    gzip_bytes,
    pax_record,
    stream_extract_gzipped_tarball,
    tempdir_with_leaked_path,
};

const MANIFEST: &[u8] = br#"{"name":"diffdemo","version":"1.0.0","main":"index.js"}"#;
const END: [u8; 1024] = [0; 1024];

fn header(name: &str, size: u64, typeflag: u8) -> Vec<u8> {
    let mut block = vec![0u8; 512];
    block[..name.len()].copy_from_slice(name.as_bytes());
    block[100..108].copy_from_slice(b"0000644\0");
    block[108..116].copy_from_slice(b"0000000\0");
    block[116..124].copy_from_slice(b"0000000\0");
    block[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    block[136..148].copy_from_slice(b"00000000000\0");
    block[156] = typeflag;
    block[257..265].copy_from_slice(b"ustar\x0000");
    seal(&mut block);
    block
}

fn seal(block: &mut [u8]) {
    block[148..156].copy_from_slice(b"        ");
    let sum: u32 = block
        .iter()
        .map(|byte| u32::from(*byte))
        .sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

fn padded(data: &[u8]) -> Vec<u8> {
    let mut bytes = data.to_vec();
    bytes.resize(data.len().div_ceil(512) * 512, 0);
    bytes
}

fn file(name: &str, data: &[u8]) -> Vec<u8> {
    [header(&format!("package/{name}"), data.len() as u64, b'0'), padded(data)].concat()
}

fn pax(typeflag: u8, records: &[(&str, &str)]) -> Vec<u8> {
    let body: String = records
        .iter()
        .map(|(key, value)| pax_record(key, value))
        .collect();
    [header("PaxHeader/x", body.len() as u64, typeflag), padded(body.as_bytes())].concat()
}

fn base() -> Vec<u8> {
    [file("package.json", MANIFEST), file("index.js", b"module.exports = 'benign';\n")].concat()
}

/// A second `package/index.js`, which a reader that lands on it after
/// misreading the previous entry's size takes for the real one.
fn hidden() -> Vec<u8> {
    file("index.js", b"module.exports = 'DIFFERENT';\n")
}

#[track_caller]
fn assert_refused(tar: &[u8], expected: &str) {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let eager = extract_tarball_entries(tar, crate::TarLimits::default(), store_dir, None);
    let streamed = stream_extract_gzipped_tarball(
        &gzip_bytes(tar),
        crate::TarLimits::default(),
        store_dir,
        None,
    );
    for (path, result) in [("eager", eager.err()), ("streamed", streamed.err())] {
        let error = result.unwrap_or_else(|| panic!("the {path} path extracted the archive"));
        assert!(format!("{error:?}").contains(expected), "{path}: {error:?}");
    }
}

fn sized_directory_tar() -> Vec<u8> {
    [
        base(),
        header("package/docs/", 512, b'5'),
        header("package/NOTES", 1024, b'0'),
        hidden(),
        END.to_vec(),
    ]
    .concat()
}

fn duplicate_pax_size_tar(first: &str, last: &str) -> Vec<u8> {
    [
        base(),
        pax(b'x', &[("size", first), ("size", last)]),
        header("package/NOTES", 1536, b'0'),
        vec![b'N'; 512],
        hidden(),
        END.to_vec(),
    ]
    .concat()
}

#[track_caller]
fn assert_index_js(tar: &[u8], expected: &[u8]) {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (eager, _) =
        extract_tarball_entries(tar, crate::TarLimits::default(), store_dir, None).unwrap();
    let (streamed, _) = stream_extract_gzipped_tarball(
        &gzip_bytes(tar),
        crate::TarLimits::default(),
        store_dir,
        None,
    )
    .unwrap();
    for files in [&eager, &streamed] {
        assert_eq!(std::fs::read(&files["index.js"]).unwrap(), expected);
    }
}

#[test]
fn reads_a_sized_directory_as_empty() {
    assert_index_js(&sized_directory_tar(), b"module.exports = 'benign';\n");
}

#[test]
fn lets_the_last_duplicate_pax_record_win() {
    assert_index_js(&duplicate_pax_size_tar("512", "1536"), b"module.exports = 'benign';\n");
    assert_index_js(&duplicate_pax_size_tar("1536", "512"), b"module.exports = 'DIFFERENT';\n");
}

#[test]
fn refuses_both_shapes_unless_reading_like_node_tar() {
    for (tar, expected) in [
        (sized_directory_tar(), "cannot hold data but has a nonzero size"),
        (duplicate_pax_size_tar("512", "1536"), "sets `size` more than once"),
    ] {
        let error = tar::Archive::new(tar.as_slice())
            .entries()
            .unwrap()
            .find_map(Result::err)
            .expect("the archive was read");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn refuses_a_link_or_device_with_a_size() {
    for typeflag in *b"2346" {
        let mut link = header("package/link", 512, typeflag);
        if typeflag == b'2' {
            link[157..163].copy_from_slice(b"target");
            seal(&mut link);
        }
        let tar = [base(), link, hidden(), END.to_vec()].concat();
        assert_refused(&tar, "cannot hold data but has a nonzero size");
    }
}

#[test]
fn refuses_a_pax_size_that_is_not_plain_digits() {
    for size in [" 512", "512.0", "0x200", "", "1e3"] {
        let tar = [
            base(),
            pax(b'x', &[("size", size)]),
            header("package/NOTES", 1536, b'0'),
            vec![b'N'; 512],
            hidden(),
            END.to_vec(),
        ]
        .concat();
        assert_refused(&tar, "invalid size");
    }
}

#[test]
fn ignores_multiline_package_metadata_and_reads_the_next_record() {
    let records =
        [("NODETAR.package.description", "first line\nsecond line"), ("path", "package/index.js")];
    let tar = [pax(b'x', &records), file("placeholder", b"contents"), END.to_vec()].concat();
    assert_index_js(&tar, b"contents");
    let error = tar::Archive::new(tar.as_slice())
        .entries()
        .unwrap()
        .next()
        .unwrap()
        .err()
        .expect("strict reader accepted multiline metadata");
    assert!(error.to_string().contains("malformed PAX extended header record"));
}

#[test]
fn refuses_ambiguous_multiline_metadata() {
    for (key, value) in [
        ("NODETAR.package.description", "first line\nordinary text\n\u{feff}  +123 text"),
        ("path", "first line\nsecond line"),
    ] {
        let tar =
            [pax(b'x', &[(key, value)]), file("placeholder", b"contents"), END.to_vec()].concat();
        assert_refused(&tar, "malformed PAX extended header record");
    }
}

#[test]
fn refuses_a_malformed_pax_record() {
    let body = format!("99 junk=1\n{}", pax_record("size", "512"));
    let tar = [
        base(),
        header("PaxHeader/x", body.len() as u64, b'x'),
        padded(body.as_bytes()),
        header("package/NOTES", 1536, b'0'),
        vec![b'N'; 512],
        hidden(),
        END.to_vec(),
    ]
    .concat();
    assert_refused(&tar, "malformed PAX extended header record");
}

#[test]
fn refuses_a_solaris_extended_header() {
    let tar = [
        base(),
        pax(b'X', &[("size", "512")]),
        header("package/NOTES", 1536, b'0'),
        vec![b'N'; 512],
        hidden(),
        END.to_vec(),
    ]
    .concat();
    assert_refused(&tar, "Solaris extended headers");
}

#[test]
fn refuses_a_global_pax_size() {
    let tar = [base(), pax(b'g', &[("size", "0")]), hidden(), END.to_vec()].concat();
    assert_refused(&tar, "global PAX extended header sets `size`");
}

#[test]
fn refuses_a_pax_header_before_an_entry_type_it_does_not_apply_to() {
    let tar = [
        base(),
        pax(b'x', &[("size", "0")]),
        header("package/NOTES", 1024, b'A'),
        hidden(),
        END.to_vec(),
    ]
    .concat();
    assert_refused(&tar, "does not apply to");
}

#[test]
fn reads_a_header_after_a_single_zero_block() {
    let tar = [base(), vec![0; 512], hidden(), END.to_vec()].concat();
    assert_index_js(&tar, b"module.exports = 'DIFFERENT';\n");
}

#[test]
fn refuses_a_regular_file_whose_path_ends_in_a_slash() {
    let tar = [base(), header("package/docs/", 1024, b'0'), hidden(), END.to_vec()].concat();
    assert_refused(&tar, "is a regular file ending in `/` with data");
}

#[test]
fn refuses_a_regular_file_with_a_link_name() {
    let mut notes = header("package/NOTES", 1024, b'0');
    notes[157..163].copy_from_slice(b"target");
    seal(&mut notes);
    let tar = [base(), notes, hidden(), END.to_vec()].concat();
    assert_refused(&tar, "is not a link but has a link name");
}

#[test]
fn refuses_an_entry_without_a_path() {
    let tar = [base(), header("", 1024, b'0'), hidden(), END.to_vec()].concat();
    assert_refused(&tar, "has no path");
}

#[test]
fn reads_a_negative_base256_size_as_no_data() {
    let mut notes = header("package/NOTES", 0, b'0');
    notes[124..136].copy_from_slice(&[0xff; 12]);
    seal(&mut notes);
    let tar = [base(), notes, hidden(), END.to_vec()].concat();
    assert_index_js(&tar, b"module.exports = 'DIFFERENT';\n");
}

#[test]
fn extracts_a_contiguous_file() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = [
        file("package.json", MANIFEST),
        header("package/index.js", 3, b'7'),
        padded(b"abc"),
        END.to_vec(),
    ]
    .concat();
    let (files, _) =
        extract_tarball_entries(&tar, crate::TarLimits::default(), store_dir, None).unwrap();
    assert_eq!(std::fs::read(&files["index.js"]).unwrap(), b"abc");
}

#[test]
fn extracts_an_archive_with_pax_paths_and_empty_directories() {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let tar = [
        file("package.json", MANIFEST),
        header("package/lib/", 0, b'5'),
        pax(b'g', &[("comment", "0123456789abcdef")]),
        pax(b'x', &[("path", "package/lib/index.js"), ("mtime", "1.5")]),
        header("ignored", 3, b'0'),
        padded(b"abc"),
        END.to_vec(),
    ]
    .concat();
    let (files, _) =
        extract_tarball_entries(&tar, crate::TarLimits::default(), store_dir, None).unwrap();
    assert_eq!(std::fs::read(&files["lib/index.js"]).unwrap(), b"abc");
}

#[test]
fn extracts_a_file_sized_by_a_pax_header() {
    let tar = [
        file("package.json", MANIFEST),
        pax(b'x', &[("size", "2")]),
        header("package/data.json", 0, b'0'),
        padded(b"{}"),
        END.to_vec(),
    ]
    .concat();
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (eager, _) =
        extract_tarball_entries(&tar, crate::TarLimits::default(), store_dir, None).unwrap();
    let (streamed, _) = stream_extract_gzipped_tarball(
        &gzip_bytes(&tar),
        crate::TarLimits::default(),
        store_dir,
        None,
    )
    .unwrap();
    for files in [&eager, &streamed] {
        assert_eq!(std::fs::read(&files["data.json"]).unwrap(), b"{}");
    }
}

/// Dropping repeated PAX keys takes linear time, so a header packed with
/// distinct keys cannot stall an install.
#[test]
fn reads_a_pax_header_with_many_distinct_keys_quickly() {
    let body: String = (0..80_000)
        .map(|index| pax_record(&format!("k{index}"), "0"))
        .collect();
    assert!(body.len() < 1024 * 1024);
    let tar = [
        file("package.json", MANIFEST),
        header("PaxHeader/x", body.len() as u64, b'x'),
        padded(body.as_bytes()),
        header("package/index.js", 3, b'0'),
        padded(b"abc"),
        END.to_vec(),
    ]
    .concat();
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let started = std::time::Instant::now();
    let (files, _) =
        extract_tarball_entries(&tar, crate::TarLimits::default(), store_dir, None).unwrap();
    assert_eq!(std::fs::read(&files["index.js"]).unwrap(), b"abc");
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
}

/// node-tar skips a metadata entry over 1 MiB without applying it.
#[test]
fn refuses_a_pax_header_node_tar_would_skip() {
    let path = format!("package/{}.js", "a".repeat(1024 * 1024));
    let tar = [base(), pax(b'x', &[("path", &path)]), hidden(), END.to_vec()].concat();
    assert_refused(&tar, "exceeds the 1048576-byte limit");
}

/// The regular files both paths extract from `tar`, which must agree, and
/// the store they are in.
#[track_caller]
fn extract_files(tar: &[u8]) -> (tempfile::TempDir, pnpm_fs::CasPaths) {
    let (store_keep, store_dir) = tempdir_with_leaked_path();
    let (eager, _) =
        extract_tarball_entries(tar, crate::TarLimits::default(), store_dir, None).unwrap();
    let (streamed, _) = stream_extract_gzipped_tarball(
        &gzip_bytes(tar),
        crate::TarLimits::default(),
        store_dir,
        None,
    )
    .unwrap();
    assert_eq!(eager, streamed);
    (store_keep, eager)
}

#[track_caller]
fn assert_file(files: &pnpm_fs::CasPaths, path: &str, expected: &[u8]) {
    let stored = files
        .get(path)
        .unwrap_or_else(|| panic!("no {path:?} in {files:?}"));
    assert_eq!(std::fs::read(stored).unwrap(), expected);
}

const BENIGN: &[u8] = b"module.exports = 'benign';\n";
const DIFFERENT: &[u8] = b"module.exports = 'DIFFERENT';\n";

#[test]
fn reads_an_old_gnu_long_name_header() {
    let name = padded(b"package/lib.js\0");
    let tar =
        [base(), header("././@LongLink", name.len() as u64, b'N'), name, hidden(), END.to_vec()]
            .concat();
    let (_store_keep, files) = extract_files(&tar);
    assert_file(&files, "index.js", BENIGN);
    assert_file(&files, "lib.js", DIFFERENT);
}

/// node-tar reads 12 checksum bytes, which run into the typeflag, and skips
/// a block whose checksum does not match.
#[test]
fn skips_a_header_whose_checksum_runs_into_the_typeflag() {
    let mut entry = hidden();
    entry[148..156].copy_from_slice(b"        ");
    let sum: u32 = entry[..512]
        .iter()
        .map(|byte| u32::from(*byte))
        .sum();
    entry[148..156].copy_from_slice(format!("{sum:08o}").as_bytes());
    let tar = [base(), entry, END.to_vec()].concat();
    let (_store_keep, files) = extract_files(&tar);
    assert_file(&files, "index.js", BENIGN);
    assert_eq!(files.len(), 2, "{files:?}");
}

/// node-tar cuts a name at its first NUL only up to a line terminator, and
/// keeps what follows.
#[test]
fn reads_a_name_with_a_line_terminator_after_a_nul() {
    for terminator in ["\n", "\r", "\u{2028}"] {
        let mut entry = hidden();
        let rest = format!("\0{terminator}");
        entry[16..16 + rest.len()].copy_from_slice(rest.as_bytes());
        entry[16 + rest.len()..100].fill(b'y');
        seal(&mut entry[..512]);
        let tar = [base(), entry, END.to_vec()].concat();
        let (_store_keep, files) = extract_files(&tar);
        assert_file(&files, "index.js", BENIGN);
        let name = format!("index.js{terminator}{}", "y".repeat(84 - rest.len()));
        assert_file(&files, &name, DIFFERENT);
    }
}

/// Node.js refuses a path with a NUL byte, so npm fails on these.
#[test]
fn refuses_a_name_that_keeps_a_nul() {
    for offset in [0, 345] {
        let mut entry = hidden();
        entry[offset + 80..offset + 84].copy_from_slice(b"x\0\ny");
        seal(&mut entry[..512]);
        let tar = [base(), entry, END.to_vec()].concat();
        assert_refused(&tar, "NUL byte in its path");
    }
}

/// The link name node-tar decodes is the one the link name checks see.
#[test]
fn refuses_a_regular_file_whose_decoded_link_name_is_not_empty() {
    let mut entry = hidden();
    entry[157..160].copy_from_slice(b"\0\nx");
    entry[160..257].fill(b'y');
    seal(&mut entry[..512]);
    let tar = [base(), entry, END.to_vec()].concat();
    assert_refused(&tar, "is not a link but has a link name");
}

/// node-tar joins an empty ustar prefix to the name when the prefix field's
/// last byte is set, and npm's `strip: 1` then drops the empty first
/// component of `/package/index.js`.
#[test]
fn joins_an_empty_ustar_prefix_when_byte_475_is_set() {
    let mut entry = hidden();
    entry[475] = b'x';
    seal(&mut entry[..512]);
    let tar = [base(), entry, END.to_vec()].concat();
    let (_store_keep, files) = extract_files(&tar);
    assert_file(&files, "index.js", BENIGN);
    assert_file(&files, "package/index.js", DIFFERENT);
}

/// JavaScript's `trim` keeps U+0085, so `parseInt` reads no number and the
/// entry has no data.
#[test]
fn reads_a_size_javascript_cannot_parse_as_no_data() {
    let mut entry = header("package/NOTES", 0, b'0');
    entry[124..136].copy_from_slice(b" \xc2\x8500002000\0");
    seal(&mut entry[..512]);
    let tar = [base(), entry, hidden(), END.to_vec()].concat();
    let (_store_keep, files) = extract_files(&tar);
    assert_file(&files, "NOTES", b"");
    assert_file(&files, "index.js", DIFFERENT);
}

/// node-tar reads the gzip members of a `.tgz` as one tar.
fn split_tgz(first_end: &[u8]) -> Vec<u8> {
    [
        gzip_bytes(&[base(), first_end.to_vec()].concat()),
        gzip_bytes(&[hidden(), END.to_vec()].concat()),
    ]
    .concat()
}

#[track_caller]
fn assert_tgz_index_js(tgz: &[u8], expected: &[u8]) {
    let (_store_keep, store_dir) = tempdir_with_leaked_path();
    let (eager, _) =
        extract_gzipped_tarball(tgz, None, crate::TarLimits::default(), store_dir, None).unwrap();
    let (streamed, _) =
        stream_extract_gzipped_tarball(tgz, crate::TarLimits::default(), store_dir, None).unwrap();
    for files in [&eager, &streamed] {
        assert_file(files, "index.js", expected);
    }
}

#[test]
fn reads_a_tar_that_continues_into_a_second_gzip_member() {
    for first_end in [&[][..], &[0; 512]] {
        assert_tgz_index_js(&split_tgz(first_end), DIFFERENT);
    }
}

#[test]
fn ignores_a_gzip_member_after_the_end_of_archive_marker() {
    assert_tgz_index_js(&split_tgz(&END), BENIGN);
}

#[test]
fn extracts_a_single_gzip_member_without_an_end_of_archive_marker() {
    assert_tgz_index_js(&gzip_bytes(&base()), BENIGN);
}

/// node-tar skips a header with a base-256 number whose first byte is
/// neither `0x80` nor `0xff`, unless a PAX record sets that field.
#[test]
fn skips_a_header_with_a_number_node_tar_cannot_decode() {
    let mut entry = hidden();
    entry[108..116].copy_from_slice(&[0x81, 0, 0, 0, 0, 0, 0, 0]);
    seal(&mut entry[..512]);
    let tar = [base(), entry.clone(), END.to_vec()].concat();
    assert_index_js(&tar, BENIGN);

    let tar = [base(), pax(b'x', &[("uid", "1000")]), entry, END.to_vec()].concat();
    assert_index_js(&tar, DIFFERENT);
}

/// Node.js refuses a name with a NUL byte, so npm fails on these.
#[test]
fn refuses_a_gnu_long_name_that_keeps_a_nul() {
    let name = b"package/lib.js\0\n\0";
    let tar = [
        base(),
        header("././@LongLink", name.len() as u64, b'L'),
        padded(name),
        hidden(),
        END.to_vec(),
    ]
    .concat();
    assert_refused(&tar, "keeps a NUL byte");
}

/// npm's `strip: 1` takes the empty first component, which leaves `./`
/// naming the package directory.
#[test]
fn reads_a_path_that_starts_with_a_slash_and_a_dot() {
    for raw in ["/./package.json", "//./package.json", "/package.json"] {
        assert_eq!(crate::clean_archive_entry_path(raw).unwrap(), "package.json", "{raw}");
    }
}

/// A gzip member may end inside an entry. node-tar reads on into the next
/// member.
fn tgz_split_inside(offset: usize) -> Vec<u8> {
    let tar = [base(), hidden(), END.to_vec()].concat();
    [gzip_bytes(&tar[..offset]), gzip_bytes(&tar[offset..])].concat()
}

#[test]
fn reads_a_gzip_member_that_ends_inside_an_entry() {
    let inside_the_last_header = base().len() + 100;
    assert_tgz_index_js(&tgz_split_inside(inside_the_last_header), DIFFERENT);
}

#[tokio::test]
async fn reads_the_manifest_of_a_gzip_member_that_ends_inside_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pkg.tgz");
    std::fs::write(&path, tgz_split_inside(530)).unwrap();
    let metadata = crate::read_local_tarball_metadata(&path, crate::IntegrityKind::Resource)
        .await
        .expect("node-tar reads the manifest across the members");
    assert_eq!(metadata.manifest.unwrap()["name"], "diffdemo");
}
