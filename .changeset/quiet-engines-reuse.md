---
"pacquet": patch
---

pnpm no longer applies the project's install settings when it installs the pnpm version the project pins. Before, a `supportedArchitectures` setting that excluded the host made pnpm skip the pinned version and keep running the installed one [pnpm/tasks#67](https://github.com/pnpm/tasks/issues/67).
