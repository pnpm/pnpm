---
"@pnpm/resolving.git-resolver": patch
"pnpm": patch
---

Resolving a private git repository no longer blocks on an interactive credential prompt: `git ls-remote` now fails fast with an authentication error when git has no credentials for the repository [#13421](https://github.com/pnpm/pnpm/issues/13421).
