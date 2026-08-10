---
"@pnpm/config.reader": minor
"pnpm": minor
---

`--config.config-dir` no longer reaches the config through a project's `pnpm-workspace.yaml`, and neither do the `--config.` spellings of the other settings a project manifest may no longer contribute (`--config.pnpm-home-dir`, `--config.workspace-dir`, `--config.global-pkg-dir`, `--config.root-project-manifest-dir`). None of them was ever a supported way to set those directories: pnpm resolves them from the environment, and these flags took effect only because the project-manifest merge re-applied the command line afterwards. The dedicated flags, such as `--dir` and `--global-dir`, are unaffected [#13629](https://github.com/pnpm/pnpm/issues/13629).
