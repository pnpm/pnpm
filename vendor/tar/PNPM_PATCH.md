# Local tar metadata bound

Source: crates.io `tar` 0.4.46, repository https://github.com/composefs/tar-rs.
Upstream revision: `fc459c149f83bf4daceaa52e17d351989002e1a9`.
The original MIT and Apache-2.0 licenses are retained.
Upstream GitHub workflow and dependency-update configuration is omitted.

This patch adds `Archive::set_max_metadata_size(Option<u64>)`. Raw and logical entry
iteration check GNU long-name, GNU long-link, and PAX payload sizes
before buffering them. GNU sparse extension chains share the same byte bound. The default remains unrestricted for compatibility.

Remove this local patch when upstream provides an equivalent metadata bound.
