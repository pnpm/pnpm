---
"@pnpm/network.git-utils": patch
"@pnpm/resolving.git-resolver": patch
"@pnpm/fetching.git-fetcher": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer appears to hang when a git dependency is fetched over SSH and ssh needs a key passphrase or a host key confirmation. The progress output hid that prompt. pnpm now runs the ssh that git spawns in batch mode, so the install fails right away with the ssh error. Load the key into an SSH agent before installing. A `GIT_SSH_COMMAND` or `GIT_SSH` environment variable set by the user is kept as is [#2227](https://github.com/pnpm/pnpm/issues/2227).
