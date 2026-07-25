---
"pacquet": patch
---

Setting both `autoInstallPeers: false` and `dedupePeerDependents: false` now leaves missing peers alone, instead of still installing the ones a version elsewhere in the workspace could satisfy.
