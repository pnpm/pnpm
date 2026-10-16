---
"pacquet": patch
---

Reduce memory usage when installing large workspaces with `sharedWorkspaceLockfile: false`. Installs waiting for workspace dependencies now count toward `workspaceConcurrency`.

Speed up independent workspace installs with scripts disabled by reusing workspace package information when no hooks or other build systems can change it.
