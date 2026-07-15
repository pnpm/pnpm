---
"pacquet": patch
---

Fixed installs failing on Windows when a symlink's parent `node_modules` was a dangling directory junction (for example, a store restored from a CI cache, which tar can't round-trip). The symlink writer now removes the dangling junction and rebuilds a real directory before linking, instead of aborting.
