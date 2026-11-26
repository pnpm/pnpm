//! Archives whose headers node-tar, which npm reads packages with, could
//! read differently from this crate. The eager and the streaming path must
//! either read them as node-tar does or refuse them.

use super::{
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
fn refuses_data_after_a_single_zero_block() {
    let tar = [base(), vec![0; 512], hidden(), END.to_vec()].concat();
    assert_refused(&tar, "data after a single zero block");
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
fn refuses_a_negative_base256_size() {
    let mut notes = header("package/NOTES", 0, b'0');
    notes[124..136].copy_from_slice(&[0xff; 12]);
    seal(&mut notes);
    let tar = [base(), notes, hidden(), END.to_vec()].concat();
    assert_refused(&tar, "invalid base-256 size");
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
