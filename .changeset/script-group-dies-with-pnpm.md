---
"@pnpm/exec.npm-lifecycle": patch
"@pnpm/prepare": patch
"pnpm": patch
"pacquet": patch
---

A script that pnpm runs without a terminal now ends when pnpm itself is killed. Killing pnpm's process group, as Playwright's `webServer` does to stop the command it started, used to leave the script running and holding the caller's output pipes open [#15555](https://github.com/pnpm/pnpm/issues/15555).
