---
"@pnpm/deps.status": patch
"pnpm": patch
---

Skip installability validation when scanning workspace projects in `checkDepsStatus` (run by `verifyDepsBeforeRun`). Previously the status check called `findWorkspaceProjects`, which validates each project's `engines` and `os`/`cpu`/`libc` and warns about useless fields in non-root manifests — work that the install pipeline already performs. With no `nodeVersion` threaded through, the engine check also fell back to the system Node from `PATH` and emitted spurious "Unsupported engine" warnings before scripts ran. Status-only callers now use `findWorkspaceProjectsNoCheck`; install paths continue to validate.
