---
"pacquet": minor
---

`pnpm doctor` now checks the environment lifecycle scripts run in. It lists the `node` executables on `PATH` and warns about broken ones, reports the shell that runs scripts, and runs a package's executable from a script in a temporary project and in the current project. If that script fails, the report includes a trace of the executable's shim that shows how it looked up and started `node` [#16308](https://github.com/pnpm/pnpm/issues/16308).
