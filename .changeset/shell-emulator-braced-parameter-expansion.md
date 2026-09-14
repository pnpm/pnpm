---
"pacquet": patch
---

Scripts run under `shellEmulator` now expand the braced parameter forms `${VAR}`, `${VAR:-default}`, and `${VAR:+alternative}`. They were passed to the script as literal text [#14814](https://github.com/pnpm/pnpm/issues/14814).
