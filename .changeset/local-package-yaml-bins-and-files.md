---
"pnpm": patch
---

`pnpm install` now links the bins of a workspace project, or of a `link:` or `file:` directory dependency, whose manifest is `package.yaml` or `package.json5`. For such a `file:` directory, it also copies only the files that the `files` field selects. It no longer skips the install scripts of such a directory. pnpm read these fields only from `package.json`.
