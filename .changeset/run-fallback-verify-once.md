---
"pacquet": patch
---

When a command name matches no script and pnpm falls back to running it as a command, the install that runs before scripts now runs once. Its errors and warnings print once [#17977](https://github.com/pnpm/pnpm/issues/17977).
