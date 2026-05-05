---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Fixed `pnpm self-update` on installations originally set up by pnpm v10. v10 added `PNPM_HOME` directly to PATH and wrote a `pnpm` bootstrap shim there. v11 setup writes shims under `PNPM_HOME/bin` instead, so when a v10 user upgrades to v11 the legacy shim at `PNPM_HOME` keeps pointing into the old `.tools/<version>` install — `pnpm --version` continues to report the pre-update version even though the new version was installed under `global/v11`. Self-update now detects this layout, refreshes the legacy shims so the upgrade actually takes effect, and prints a hint suggesting `pnpm setup` to migrate PATH to the v11 layout. [#11464](https://github.com/pnpm/pnpm/issues/11464).
