---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `peerDependencyRules` or any of its nested fields (`ignoreMissing`, `allowAny`, `allowedVersions`) in `pnpm-workspace.yaml` has an invalid type.
