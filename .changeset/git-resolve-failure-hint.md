---
"@pnpm/resolving.git-resolver": patch
"pacquet": patch
"pnpm": patch
---

A git dependency whose `git ls-remote` fails now reports the `ERR_PNPM_GIT_RESOLVE_FAILED` code, naming the dependency instead of printing a bare `git` invocation, with credentials in the repository URL redacted. A specifier that does not ask for SSH resolves over HTTPS, because the URL recorded in the lockfile has to work on every machine that installs it, so the error explains how to substitute the transport on a machine that can only reach the host over SSH (`git config --global url."git@<host>:".insteadOf "https://<host>/"`) [#13743](https://github.com/pnpm/pnpm/issues/13743).

A missing `git` executable is reported as one, instead of surfacing the raw failure to start the process.

Credentials embedded in a git specifier are redacted from the "Could not resolve \<ref\> to a commit of \<repo\>" errors too.

Resolving a public repository makes one `git ls-remote` round-trip instead of two.
