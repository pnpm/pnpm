#!/usr/bin/env bash
# `RUSTDOC` for the pre-push doc check. By default every rustdoc invocation
# rewrites the search index shared by all crates in `target/doc` under one
# lock, which costs ~10s per crate once the workspace's index is large.
# `--merge=none` skips that write and keeps every rustdoc diagnostic.
RUSTC_BOOTSTRAP=1 exec rustdoc -Zunstable-options --merge=none "$@"
