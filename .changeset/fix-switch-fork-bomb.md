---
"@pnpm/exec.pnpm-cli-runner": minor
"@pnpm/tools.plugin-commands-self-updater": patch
"pnpm": patch
---

Fix an infinite fork-bomb that could happen when pnpm was installed with one version (e.g. `npm install -g pnpm@A`) and run inside a project whose `package.json` selected a different pnpm version via the `packageManager` field (e.g. `pnpm@B`), while a `pnpm-workspace.yaml` also existed at the project root.

The child process spawned by `installPnpmToTools` (to install the wanted pnpm version) inherited the parent's environment but had its working directory set to a fresh stage dir. pnpm's workspace walk-up from that stage dir would find the ancestor `pnpm-workspace.yaml` at the project root, adopt the root `package.json`, re-trigger `switchCliVersion`, and call `installPnpmToTools` again — recursively. Because the target tool dir isn't symlinked in until the outer install completes, each recursive call saw `alreadyExisted === false` and started another nested install, fork-bombing the process tree at ~100% CPU.

The child's environment is now forced to `manage-package-manager-versions=false` (v10) and `pm-on-fail=ignore` (v11+), which disables the package-manager-version handling in whichever pnpm runs as the child.

Fixes [#11337](https://github.com/pnpm/pnpm/issues/11337).
