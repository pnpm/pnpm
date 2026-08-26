---
"pacquet": patch
---

pnpm now runs the pnpm version that its lockfile resolved a `devEngines.packageManager` range to. A project pinning `^12.0.0-rc.3` whose `pnpm-lock.yaml` records `12.0.0-rc.11` kept running whatever pnpm was on `PATH` as long as it satisfied the range too, so contributors silently ran different versions of pnpm; the recorded resolution now decides, as it does in the TypeScript CLI.

Version pins are also matched the way npm's `semver` matches them with `includePrerelease`. A prerelease no longer counts as satisfying a range asking for something later — `12.0.0-rc.7` against `>=12.0.0-rc.9` or `^12.0.0` — and a bound that leaves a component out is read as npm reads it, so a `<=22` engine range accepts 22.5.0. This applies to the package manager check and to `engines` checks alike.
