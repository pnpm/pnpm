---
"@pnpm/workspace.workspace-manifest-reader": patch
"pnpm": patch
---

A falsy non-array `packages` field in `pnpm-workspace.yaml`, such as `packages: false`, is now rejected with an error instead of being treated as omitted.
