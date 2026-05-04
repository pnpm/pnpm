---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

The `WARN` and error code labels in pnpm's output now wrap in brackets (`[WARN]`, `[ERR_PNPM_FOO]`). Previously the labels relied entirely on a colored background to stand out, which meant they blended into the surrounding text in terminals without color (e.g. when `NO_COLOR` is set or output is piped). The brackets are painted in the same color as the badge background, so they appear as ordinary padding in color-capable terminals — only the no-color rendering changes.
