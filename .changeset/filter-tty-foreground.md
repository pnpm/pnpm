---
"pacquet": patch
---

Fixed `pnpm --filter <package> <script>` hanging when the script reads from the terminal. Scripts that cannot run alongside another script now stay in the terminal's foreground process group, so interactive prompts work again [#14397](https://github.com/pnpm/pnpm/issues/14397).
