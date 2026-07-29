---
"pacquet": patch
---

Arguments after the script or command name now reach the script untouched for `pnpm run`, `pnpm exec`, `pnpm dlx`, and `pnpm with`, matching the JavaScript implementation. Previously `pnpm run build --config.foo=bar` consumed the argument as a pnpm setting instead of forwarding it, and `pnpm run build --silent` handed the script `--reporter=silent` — a token the user never typed [#13302](https://github.com/pnpm/pnpm/issues/13302). Put such flags before the script name (`pnpm run --silent build`) to apply them to pnpm.
