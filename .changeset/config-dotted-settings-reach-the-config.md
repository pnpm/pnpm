---
"pacquet": patch
---

Every setting pnpm supports can now be set with `--config.<name>=<value>` on the command line, not only the ones whose command also carries a matching flag. Before, `pnpm install --config.frozen-lockfile=true` dropped the setting and rewrote `pnpm-lock.yaml` as though the install had not been frozen [#16276](https://github.com/pnpm/pnpm/issues/16276).
