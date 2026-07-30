---
"pacquet": patch
---

Fresh installs no longer download the tarballs of platform-specific optional dependencies that don't match the current platform. Registry manifests list such packages in both `dependencies` and `optionalDependencies` (npm merges the two at publish time), and the resolver followed the duplicate as a non-optional dependency, bypassing the platform check that skips the download.
