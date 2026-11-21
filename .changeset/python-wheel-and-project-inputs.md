---
"pacquet": patch
---

Fixed Python wheel installs with legacy metadata, whitespace around entry point targets, and interpreter options in script shebangs.

Python dependency groups now match normalized names. Adding a development dependency reports when the project excludes the dev group.

Comments ending in a backslash in `requirements.txt` no longer hide the next requirement. `pnpm exec` finds the environment of requirements-only projects under a tool-only `pyproject.toml`.

Fixed `pnpm add` in shared Python workspaces that contain a requirements-only member.
