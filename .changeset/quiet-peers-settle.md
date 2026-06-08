---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Made peer-dependent deduplication deterministic. When a peer-suffixed package variant was a subset of two or more mutually incompatible larger variants, the variant it collapsed into depended on the order importers were resolved in, which varies between machines. This could resolve the same workspace to different lockfiles on different platforms and make `pnpm dedupe --check` alternate between passing and failing.
