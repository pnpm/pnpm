---
"pnpm": patch
"pacquet": patch
---

In a project that pins another pnpm version, pnpm now passes a command with an option it does not know to the pinned version. Before, pnpm rejected the option before switching, so `pnpm install --auto-dedupe` failed with "Unknown option" even though the pinned pnpm supports it [#16353](https://github.com/pnpm/pnpm/issues/16353).
