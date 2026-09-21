---
"@pnpm/network.git-utils": patch
"@pnpm/resolving.git-resolver": patch
"@pnpm/fetching.git-fetcher": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer appears to hang when a git dependency is fetched over SSH and ssh asks for a key passphrase or a host key confirmation. pnpm now runs ssh in batch mode, so the install fails right away with the ssh error, and a key that needs a passphrase has to be loaded into an SSH agent first. An ssh command selected through `GIT_SSH_COMMAND`, `GIT_SSH`, or the `core.sshCommand` git setting is kept as is [#2227](https://github.com/pnpm/pnpm/issues/2227).
