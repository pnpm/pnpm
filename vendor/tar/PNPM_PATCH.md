# Local tar patches

Source: crates.io `tar` 0.4.46, repository https://github.com/composefs/tar-rs.
Upstream revision: `fc459c149f83bf4daceaa52e17d351989002e1a9`.
The original MIT and Apache-2.0 licenses are retained.
Upstream GitHub workflow and dependency-update configuration is omitted.

`Cargo.toml` also caps `filetime` below 0.2.30; the comment there says why and when to drop the cap.

## Metadata bound

`Archive::set_max_metadata_size(Option<u64>)` makes raw and logical entry
iteration check GNU long-name, GNU long-link, and PAX payload sizes before
buffering them. The default remains unrestricted for compatibility.

Remove this patch when upstream provides an equivalent metadata bound.

## Agreement with node-tar

`src/agreement.rs` rejects every header shape that node-tar, the reader npm,
Yarn, and Bun use, would read differently from this crate. Each check there
documents the disagreement it closes. Entry iteration calls them from
`src/archive.rs` and `Header::entry_size`.
Upstream tests that build those shapes now fail on purpose.
`Archive::set_read_like_node_tar(true)` reads two of the shapes as node-tar
does instead: a sized directory and a repeated PAX key.

`EntryType::is_file` also counts contiguous files, which node-tar extracts as
regular files.

Keep this patch for as long as pnpm reads npm packages with this crate.
