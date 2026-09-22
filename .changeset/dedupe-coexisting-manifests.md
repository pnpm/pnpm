---
"@pnpm/workspace.projects-reader": patch
"pnpm": patch
---

Fixed workspace discovery returning duplicate projects for a single directory. When several manifest formats coexist in the same package directory (for example both `package.json` and `package.json5`), `findPackages` matched all of them and produced one project per file instead of one per directory. A single manifest is now selected per directory, using the same `package.json` > `package.json5` > `package.yaml` precedence that `readProjectManifest` already applies.
