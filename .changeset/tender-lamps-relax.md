---
"pnpm": minor
---

There is no need to escape the command shell with `--`, when using the exec command. So just `pnpm exec rm -rf dir` instead of `pnpm exec -- rm -rf dir`.
