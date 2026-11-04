---
"pacquet": minor
---

`pnpm -r publish --batch` publishes packages that declare `publishConfig.variants` to a registry declared `serverType: pnpr`. pnpm stages every package's archives first, including archives that other jobs uploaded with `--upload-variant`. The batch then commits them by reference.
