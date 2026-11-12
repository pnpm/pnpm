use super::LockfileEntries;
use pnpm_lockfile::Lockfile;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::path::Path;
use text_block_macros::text_block;

const SHA512: &str = "sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==";
const SHA1: &str = "sha1-dTU0W4lnNNX4DE0GxQlVUnoU8Ss=";

fn entries(lockfile: &str) -> LockfileEntries {
    let lockfile = Lockfile::parse(lockfile, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("a lockfile document");
    LockfileEntries::from_lockfile(&lockfile)
}

fn keys_for(entries: &LockfileEntries, integrity: &str) -> Vec<String> {
    let integrity: Integrity = integrity.parse().expect("parse the integrity");
    let mut keys: Vec<String> = entries
        .store_index_keys([&integrity])
        .map(str::to_owned)
        .collect();
    keys.sort();
    keys
}

#[test]
fn each_entry_pinning_a_hash_is_keyed_by_its_own_id() {
    let entries = entries(text_block! {
        "lockfileVersion: '9.0'"
        "packages:"
        "  is-number@7.0.0:"
        "    resolution: {integrity: sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==}"
        "  is-number@https://tarballs.example/is-number.tgz:"
        "    resolution: {integrity: sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==, tarball: https://tarballs.example/is-number.tgz}"
        "    version: 7.0.0"
        "  is-number@https://codeload.github.com/acme/is-number/tar.gz/98e8ff1da1a89f93d1397a24d7413ed15421c139:"
        "    resolution: {gitHosted: true, integrity: sha512-41Cifkg6e8TylSpdtTpeLVMqvSBEVzTttHvERD741+pnZ8ANv0004MRL43QKPDlK9cGvNp6NZWZUBlbGXYxxng==, tarball: https://codeload.github.com/acme/is-number/tar.gz/98e8ff1da1a89f93d1397a24d7413ed15421c139}"
        "    version: 7.0.0"
    });
    assert_eq!(
        keys_for(&entries, SHA512),
        [
            format!(
                "{SHA512}\thttps://codeload.github.com/acme/is-number/tar.gz/98e8ff1da1a89f93d1397a24d7413ed15421c139"
            ),
            format!("{SHA512}\thttps://tarballs.example/is-number.tgz"),
            format!("{SHA512}\tis-number@7.0.0"),
        ],
    );
}

#[test]
fn an_entry_is_keyed_by_the_integrity_it_records() {
    let entries = entries(text_block! {
        "lockfileVersion: '9.0'"
        "packages:"
        "  is-number@7.0.0:"
        "    resolution: {integrity: sha1-dTU0W4lnNNX4DE0GxQlVUnoU8Ss=}"
    });
    assert_eq!(keys_for(&entries, SHA1), [format!("{SHA1}\tis-number@7.0.0")]);
    assert_eq!(keys_for(&entries, SHA512), Vec::<String>::new());
}

#[test]
fn an_entry_without_an_integrity_is_not_keyed() {
    let entries = entries(text_block! {
        "lockfileVersion: '9.0'"
        "packages:"
        "  is-number@https://tarballs.example/is-number.tgz:"
        "    resolution: {tarball: https://tarballs.example/is-number.tgz}"
        "    version: 7.0.0"
    });
    assert_eq!(keys_for(&entries, SHA512), Vec::<String>::new());
}
