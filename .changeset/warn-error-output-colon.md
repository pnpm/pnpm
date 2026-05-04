---
"@pnpm/cli.default-reporter": minor
"pnpm": minor
---

The `WARN` and error code labels in pnpm's output now end with a colon (e.g. `WARN:`, `ERR_PNPM_FOO:`). Previously the labels relied entirely on a colored background to stand out, which meant they blended into the surrounding text in terminals without color (e.g. when `NO_COLOR` is set or output is piped). The colored badge is still applied on top in color-capable terminals, so this is a no-op there.
