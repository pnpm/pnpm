---
"pacquet": patch
---

Under `nodeLinker: isolated`, a Bit root-component member whose materialized slot carries no `package.json` now gets its sibling symlinks from its own lockfile snapshot instead of gaining a link to every other member of the root. On a Bit workspace the all-member fallback cost one symlink per (member, member) pair — hundreds of thousands of inodes on a large workspace — while the snapshot declares the same edges exactly. The all-member fallback remains only for a member with neither a manifest nor a snapshot.
