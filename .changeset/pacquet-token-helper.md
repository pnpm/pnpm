---
"pacquet": minor
---

Added support for the `tokenHelper` auth setting, matching the TypeScript CLI. A `tokenHelper` configured in `~/.npmrc` or the global pnpm `auth.ini` names a command pacquet runs to obtain a registry token; the command runs lazily (only when a request to that registry is actually made) and its output becomes the `Authorization` header. A `tokenHelper` in a workspace or project `.npmrc`, or supplied through a URL-scoped environment variable, is refused so a checked-in config can't run an arbitrary command.
