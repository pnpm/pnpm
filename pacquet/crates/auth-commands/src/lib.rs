//! Commands for authenticating with npm registries — pacquet's port of
//! pnpm's [`@pnpm/auth.commands`](https://github.com/pnpm/pnpm/tree/main/pnpm11/auth/commands).
//!
//! Only `logout` is ported so far.

pub mod logout;

mod ini;
