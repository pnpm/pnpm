---
"pacquet": minor
---

Inside a workspace, `pnpm create <name>` runs the workspace project named `create-<name>` or `@scope/create-<name>` when one exists. The project's dependencies must already be installed. A name with a version or tag, such as `pnpm create foo@latest`, still fetches the package from the registry [#4242](https://github.com/pnpm/pnpm/issues/4242).
