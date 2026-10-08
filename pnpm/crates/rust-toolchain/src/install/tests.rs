use super::{newest_installed, toolchain_dir, unpack};
use crate::{Channel, Profile, ToolchainRequest};
use flate2::{Compression, write::GzEncoder};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

const HOST: &str = "x86_64-unknown-linux-gnu";

enum Entry<'a> {
    File(&'a str, &'a [u8], u32),
    Symlink(&'a str, &'a str),
    RawPath(&'a str),
}

fn gzipped_tar(entries: &[Entry<'_>]) -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        match entry {
            Entry::File(path, contents, mode) => {
                header.set_size(contents.len() as u64);
                header.set_mode(*mode);
                header.set_entry_type(tar::EntryType::Regular);
                builder.append_data(&mut header, path, *contents).unwrap();
            }
            Entry::Symlink(path, target) => {
                header.set_size(0);
                header.set_entry_type(tar::EntryType::Symlink);
                builder.append_link(&mut header, path, target).unwrap();
            }
            Entry::RawPath(path) => {
                header.set_size(0);
                header.set_entry_type(tar::EntryType::Regular);
                header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
                header.set_cksum();
                builder
                    .append(&header, &[][..])
                    .unwrap();
            }
        }
    }
    builder
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
}

fn request(channel: &str, components: &[&str]) -> ToolchainRequest {
    ToolchainRequest {
        channel: Channel::parse(channel).unwrap(),
        profile: Profile::Default,
        components: components
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        targets: Vec::new(),
    }
}

#[test]
fn unpacks_the_component_directory_without_the_installer() {
    let destination = tempfile::tempdir().unwrap();
    let archive = gzipped_tar(&[
        Entry::File("rustc-1.90.0-x86_64-unknown-linux-gnu/install.sh", b"#!/bin/sh", 0o755),
        Entry::File("rustc-1.90.0-x86_64-unknown-linux-gnu/components", b"rustc", 0o644),
        Entry::File(
            "rustc-1.90.0-x86_64-unknown-linux-gnu/rustc/manifest.in",
            b"file:bin/rustc",
            0o644,
        ),
        Entry::File("rustc-1.90.0-x86_64-unknown-linux-gnu/rustc/bin/rustc", b"rustc", 0o755),
        Entry::File(
            "rustc-1.90.0-x86_64-unknown-linux-gnu/rustc/lib/librustc_driver.so",
            b"driver",
            0o644,
        ),
    ]);

    unpack(&archive, destination.path()).unwrap();

    let mut files = walk(destination.path());
    files.sort();
    assert_eq!(files, ["bin/rustc", "lib/librustc_driver.so"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(destination.path().join("bin/rustc"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn refuses_entries_that_are_not_regular_files() {
    let destination = tempfile::tempdir().unwrap();
    let archive = gzipped_tar(&[Entry::Symlink("rustc-1.90.0/rustc/bin/rustc", "/bin/sh")]);
    let error = unpack(&archive, destination.path()).expect_err("a symlink is refused");
    eprintln!("{error}");
    assert!(walk(destination.path()).is_empty());
}

#[test]
fn refuses_entries_that_leave_the_toolchain() {
    let destination = tempfile::tempdir().unwrap();
    let archive = gzipped_tar(&[Entry::RawPath("rustc-1.90.0/rustc/../../../escaped")]);
    let error = unpack(&archive, destination.path()).expect_err("a traversal is refused");
    eprintln!("{error}");
    assert!(walk(destination.path()).is_empty());
}

#[test]
fn a_toolchain_directory_names_its_release_and_selection() {
    let store = Path::new("/store/rust");
    let pinned = Channel::parse("1.90.0").unwrap();
    let default = toolchain_dir(store, &pinned, HOST, &request("stable", &[]));
    let with_source = toolchain_dir(store, &pinned, HOST, &request("1.90.0", &["rust-src"]));
    let name = default
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    assert!(name.starts_with("1.90.0-x86_64-unknown-linux-gnu-"), "{name}");
    assert_ne!(default, with_source);
    assert_eq!(default, toolchain_dir(store, &pinned, HOST, &request("1.90", &[])));
}

#[test]
fn offline_a_moving_channel_uses_its_newest_installed_release() {
    let store = tempfile::tempdir().unwrap();
    let stable = request("stable", &[]);
    for version in ["1.89.0", "1.90.0", "1.100.0"] {
        fs::create_dir_all(toolchain_dir(
            store.path(),
            &Channel::parse(version).unwrap(),
            HOST,
            &stable,
        ))
        .unwrap();
    }
    fs::create_dir_all(toolchain_dir(
        store.path(),
        &Channel::parse("nightly-2030-01-01").unwrap(),
        HOST,
        &stable,
    ))
    .unwrap();

    assert_eq!(
        newest_installed(store.path(), HOST, &stable),
        Some(toolchain_dir(store.path(), &Channel::parse("1.100.0").unwrap(), HOST, &stable)),
    );
    assert_eq!(
        newest_installed(store.path(), HOST, &request("1.89", &[])),
        Some(toolchain_dir(store.path(), &Channel::parse("1.89.0").unwrap(), HOST, &stable)),
    );
    assert_eq!(newest_installed(store.path(), HOST, &request("stable", &["rust-src"])), None);
}

fn walk(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap();
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files
}
