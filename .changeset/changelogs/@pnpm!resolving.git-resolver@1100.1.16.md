## 1100.1.16

### Patch Changes

- Fixed a CI regression where `github:owner/repo` dependencies (and other shorthand Git specifiers) would fail to install with `Permission denied (publickey)` on CI runners that lack SSH keys. The Git resolver no longer records an SSH URL unless the user explicitly wrote one (e.g. `git+ssh://` or `git@host:...`):

  - The repository visibility probe (an HTTP HEAD request) now retries transient failures such as `429 Too Many Requests`, so host throttling of CI runners is no longer mistaken for a private repository.
  - For non-SSH specifiers, anonymous HTTPS `git ls-remote` access is now tried before SSH, so a public repository whose visibility probe fails still resolves to a portable HTTPS URL instead of an SSH URL that only works where SSH keys are configured.
  - When every probe fails, the resolver falls back to HTTPS for shorthand and HTTPS-style specifiers, and only guesses SSH when the user explicitly provided an SSH URL.
  - A repository that could not be confirmed public is no longer resolved to the host's anonymous archive URL (e.g. `codeload.github.com`, which would fail to download for a private repository); it stays a regular `git` resolution so installs can use ambient Git credentials such as credential helpers and tokens.

  Note that a private repository that is reachable both over authenticated HTTPS and over SSH now resolves to its HTTPS URL, where previous versions recorded the SSH URL.

  Fixes [pnpm/pnpm#13276](https://github.com/pnpm/pnpm/issues/13276).

  <!-- cspell:ignore publickey -->

- Resolving a private git repository no longer blocks on an interactive credential prompt: `git ls-remote` now fails fast with an authentication error when git has no credentials for the repository [#13522](https://github.com/pnpm/pnpm/issues/13522).
