---
"pacquet": patch
---

`pnpm outdated` and `pnpm update` now include Cargo dependencies declared under `[target.<cfg>.dependencies]`. `pnpm update --prod` no longer fails on a crate that is also a dev-dependency. `pnpm add crate:` fails in a virtual Cargo workspace root, as `cargo add` does. It added a `[dependencies]` table that Cargo rejects there.
