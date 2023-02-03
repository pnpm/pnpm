---
"@pnpm/npm-resolver": patch
"@pnpm/core": patch
"pnpm": patch
---

Deduplicate direct dependencies.

Let's say there are two projects in the workspace that dependend on `foo`. One project has `foo@1.0.0` in the dependencies while another one has `foo@^1.0.0` in the dependencies. In this case, `foo@1.0.0` should be installed to both projects as satisfies the version specs of both projects.
