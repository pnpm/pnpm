---
"@pnpm/config.reader": minor
"pnpm": minor
---

`--config.config-dir` no longer changes anything. It only ever took effect inside a workspace, and even there it moved only what pnpm touches *after* the config is loaded — where `pnpm login` writes the granted token, where `pnpm logout` deletes it, and which file `pnpm config set --global` and `pnpm config get --global` target — while pnpm went on reading `config.yaml` and the user's `auth.ini` from the real config directory. That directory now comes from the environment (`XDG_CONFIG_HOME`, or the platform default) everywhere, which is where every read already took it from [#13629](https://github.com/pnpm/pnpm/issues/13629).
