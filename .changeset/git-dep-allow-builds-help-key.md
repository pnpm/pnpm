---
"pacquet": patch
---

When a git-hosted dependency is blocked from running build scripts, the error now suggests an `allowBuilds` entry that actually approves it. It quoted the bare package name, which never matches a git-hosted package, so following the suggestion left the install failing the same way [#14002](https://github.com/pnpm/pnpm/issues/14002).
