---
"@pnpm/config.reader": minor
"pnpm": minor
---

`--config.config-dir` no longer changes anything. It only ever took effect inside a workspace, and even there it moved only what pnpm touches *after* the config is loaded — where `pnpm login` writes the granted token, where `pnpm logout` deletes it, and which file `pnpm config set --global` and `pnpm config get --global` target — while pnpm went on reading `config.yaml` and the user's `auth.ini` from the real config directory. That directory now comes from the environment (`XDG_CONFIG_HOME`, or the platform default) everywhere, which is where every read already took it from. The same applies to the `--config.` escape hatch for the other settings a project manifest can no longer contribute — `--config.pnpm-home-dir`, `--config.workspace-dir`, `--config.global-pkg-dir` and `--config.root-project-manifest-dir` reached the config through that merge and are now inert too. The dedicated flags are unaffected: `--dir`, `--global-dir`, `--global-bin-dir`, `--state-dir`, `--userconfig` and `--npmrc-auth-file` apply earlier and still win [#13629](https://github.com/pnpm/pnpm/issues/13629).
