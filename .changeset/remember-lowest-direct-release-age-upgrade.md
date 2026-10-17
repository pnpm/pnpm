---
"pacquet": patch
---

With `minimumReleaseAge` set and `resolutionMode` set to `lowest-direct` or `time-based`, pnpm no longer downloads a direct dependency's full metadata again for every project that depends on it. This happened when the registry's metadata did not list a publish time for every version.
