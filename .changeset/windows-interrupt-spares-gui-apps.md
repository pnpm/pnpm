---
"pacquet": patch
---

On Windows, pressing `Ctrl+C` or closing the terminal during `pnpm run` or `pnpm exec` no longer ends programs that a script started outside the terminal, such as a browser opened by the `open` package. Processes attached to the terminal are still ended [#17465](https://github.com/pnpm/pnpm/issues/17465).
