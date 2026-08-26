---
"pacquet": patch
---

pnpm now runs the pnpm version that `pnpm-lock.yaml` records for a `devEngines.packageManager` range, instead of any pnpm on `PATH` the range also allows. A project pinning `^12.0.0-rc.3` with `12.0.0-rc.11` recorded went on running an older `12.0.0-rc.7`.

Version pins are also matched the way npm's `semver` matches them: a prerelease no longer counts as satisfying a range asking for something later — `12.0.0-rc.7` against `>=12.0.0-rc.9` or `^12.0.0` — and a bound that omits a component is read as npm reads it, so a `<=22` engine range accepts 22.5.0. This applies to the package manager check and to `engines` checks alike.
