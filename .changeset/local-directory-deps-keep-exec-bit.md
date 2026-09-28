---
"pacquet": patch
---

Files of a `file:` directory dependency or an injected workspace package now keep the permissions they have in their project. Since 12.8.0, an executable file in such a dependency was installed without its executable bit.
