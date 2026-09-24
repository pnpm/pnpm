---
"pacquet": patch
---

Approving immature packages at the minimum-release-age prompt during a package-manager engine install (for example, a `devEngines.packageManager` version switch) now records the approved entries in the invoking project's `pnpm-workspace.yaml` under `minimumReleaseAgeExclude`, as the prompt promises. Previously the entries were written to the throwaway engine-install directory, which is deleted right after the install, so the approval was silently lost and the prompt reappeared on every command. Related to https://github.com/pnpm/pnpm/issues/15396.
