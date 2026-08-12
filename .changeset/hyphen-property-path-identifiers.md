---
"@pnpm/object.property-path": patch
"pnpm": patch
"pacquet": patch
---

Allow hyphens in property path identifiers so `pnpm pkg get/set` accepts package names like `dependencies.some-package-name` (GH-13163).
