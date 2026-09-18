---
"pacquet": patch
---

`pnpm install` no longer fails with "Too many levels of symbolic links" when a Cargo configuration file above the workspace is a symlink, such as a `~/.cargo/config.toml` linked from a dotfiles repository.
