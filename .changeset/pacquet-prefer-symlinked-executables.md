---
"pacquet": minor
---

Added support for the `preferSymlinkedExecutables` setting. On POSIX systems, `node_modules/.bin` entries are created as symlinks to the executable files instead of shell shims, and `NODE_PATH` pointing at the virtual store of the workspace root is exported to spawned scripts so they can resolve dependencies from the hoisted store. Like the TypeScript CLI, the setting turns on automatically when `nodeLinker` is set to `hoisted`.
