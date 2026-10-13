---
"pacquet": patch
---

An install that fetches into the store without writing `node_modules` (`enableModulesDir: false`) verified the packages the store already held only after the dependencies were resolved, and checked the files of a large package one after another. The verification now runs while the dependencies resolve, and checks a package's files in parallel. On a 5,100-package install with the store on a network filesystem, this removed a 19-second pause between the resolution and the fetch.
