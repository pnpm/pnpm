---
"pacquet": patch
---

With `nodeLinker: hoisted` and `autoInstallPeers: false`, a package whose peer dependency its parent supplies at a version other than the top-level one now stays nested under that parent, even when a copy of the same package version is already at the top level. Before, the nested copy was removed as a duplicate, so the package resolved its peer to the top-level version.
