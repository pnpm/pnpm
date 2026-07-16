---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Fixed `pnpm self-update` crashing with `Cannot use 'in' operator to search for 'integrity' in undefined` when the global environment lockfile recorded a dependency with a peer suffix (e.g. `fdir@6.5.0(picomatch@4.0.5)`). The temporary lockfile built for the update looked the package's resolution up under its full `snapshots` key, but `packages` is keyed without the peers suffix, so the resolution was dropped [#12959](https://github.com/pnpm/pnpm/issues/12959).
