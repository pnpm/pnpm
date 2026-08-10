---
"@pnpm/resolving.git-resolver": patch
"pacquet": patch
"pnpm": patch
---

A git dependency whose remote `git ls-remote` cannot reach now fails with the `ERR_PNPM_GIT_RESOLVE_FAILED` code, naming the dependency instead of printing a bare `git` invocation, with credentials in the repository URL redacted. A specifier that does not ask for SSH resolves over HTTPS, because the URL recorded in the lockfile has to work on every machine that installs it, so the error explains how to substitute the transport on a machine that can only reach the host over SSH (`git config --global url."git@<host>:".insteadOf "https://<host>/"`) [#13743](https://github.com/pnpm/pnpm/issues/13743).

A public repository is no longer probed with an extra `git ls-remote` before resolution: the probe's outcome could not change what gets recorded, and skipping it saves a round-trip per git dependency.
