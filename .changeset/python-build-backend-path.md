---
"pacquet": patch
---

`pnpm install` now puts the executables of a Python build backend's requirements on the `PATH` while the backend runs. Projects built with `uv_build` now install. Before, they failed with "uv-build was not properly installed".
