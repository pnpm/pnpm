---
"pacquet": minor
---

Added support for the `syncInjectedDepsAfterScripts` setting. It names the scripts after which every injected copy of the package that ran them is brought back in step with its source, so a build script no longer leaves the copies in the virtual store holding stale files.
