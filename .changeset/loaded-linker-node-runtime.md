---
"pacquet": patch
---

With `nodeLinker.type: loaded`, scripts can now run a Node.js runtime installed through `devEngines.runtime`. Before this fix, every script that called `node` re-ran its own shim until it failed with "Argument list too long".
