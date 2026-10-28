---
"pacquet": patch
---

`pnpm install` now creates `node_modules/.pnpm-workspace-state-v1.json` with the same permissions as other new files. It was readable only by the user who ran the install, so a different user running `pnpm run` reinstalled the dependencies. This happened, for example, in a Docker image built as root and run as a non-root user [#17435](https://github.com/pnpm/pnpm/discussions/17435).
