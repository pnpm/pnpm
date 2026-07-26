---
"pacquet": patch
---

The root project's `pnpm:devPreinstall` script now runs at the start of every install that isn't `--ignore-scripts`, before resolution and linking. Workspaces that use the hook to prepare state the install depends on — such as [next.js](https://github.com/vercel/next.js), which generates a placeholder `next` bin with it — were left with dependents linked against files that were never created [#13313](https://github.com/pnpm/pnpm/issues/13313).
