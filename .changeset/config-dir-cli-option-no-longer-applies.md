---
"@pnpm/config.reader": minor
"pnpm": minor
---

`--config.config-dir` no longer changes anything. It only ever took effect inside a workspace, and even there it moved just one thing — the `auth.ini` that `pnpm login` writes the granted token to — while pnpm went on reading `config.yaml` and `auth.ini` from the real config directory. That directory now comes from the environment (`XDG_CONFIG_HOME`, or the platform default) everywhere, which is where every read already took it from [#13629](https://github.com/pnpm/pnpm/issues/13629).
