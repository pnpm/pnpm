---
"pacquet": patch
---

When pnpm switches to the version a project pins, it now installs that version's native binary for the current host, even if the project's `supportedArchitectures` excludes the host. Before, pnpm could not use the pinned version and kept running the installed one. pnpm also repairs a pinned version that an earlier install left without the host's binary.
