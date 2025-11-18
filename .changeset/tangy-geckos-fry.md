---
"@pnpm/plugin-commands-installation": major
"@pnpm/parse-cli-args": major
"pnpm": major
---

Support lowercase options in `pnpm add`: `-d`, `-p`, `-o`, `-e` [#9197](https://github.com/pnpm/pnpm/issues/9197).

When using `pnpm add` command only:

- `-p` is now an alias for `--save-prod` instead of `--parseable`
- `-d` is now an alias for `--save-dev` instead of `--loglevel=info`
