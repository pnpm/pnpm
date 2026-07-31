---
"@pnpm/store.path": patch
"pnpm": patch
---

Fix store path selection when only the project directory is writable, preventing pnpm from creating a project-local `.pnpm-store`.
