---
"pacquet": patch
---

Previously, on Windows, a failed command, `Ctrl+C`, or a closed terminal ended every program a script had started, such as a browser opened by the `open` package. Processes that a script or `pnpm exec` command leaves running now outlive pnpm exactly when they would outlive the command run without pnpm. pnpm still ends them when it is killed or crashes.

Previously, a bail killed the scripts still running in a recursive or parallel run at once. On Unix they now receive `SIGTERM` and are killed only if they are still running two seconds later. On Windows, everything those scripts started is ended, including detached processes whose parent has already exited [#17465](https://github.com/pnpm/pnpm/issues/17465).
