---
"pacquet": minor
---

`pnpm doctor` now checks the environment that lifecycle scripts run in. It lists the `node` executables on `PATH` and warns about broken ones. It reports the shell that runs scripts. A new check runs a package's executable from a script, both in a temporary project and in the current project. If that script fails, the report includes the error and, outside Windows, a trace of the executable's shim that shows how it looked up and started `node` [#16308](https://github.com/pnpm/pnpm/issues/16308).
