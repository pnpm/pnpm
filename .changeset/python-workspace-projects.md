---
"pacquet": minor
---

`pnpm install` now installs a Python project in the workspace from its own source. Declare it under `[tool.uv.sources]`, as `shared = { workspace = true }` or `shared = { path = "../shared", editable = true }`. pnpm builds the project with the backend it declares. It installs the build editable, so an edit to the project takes effect without another install.

Approve the build backend under `allowBuilds` in `pnpm-workspace.yaml` as a Package URL, as `pkg:pypi/hatchling: true`. An install that has not approved a backend does not build the projects that need it. The message names the key to add.

`pnpm install` now refuses a requirement that names a project in the workspace when nothing declares where it comes from. It used to take that name from the index.
