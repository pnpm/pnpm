---
"pacquet": patch
---

Sped up installs in large workspaces by resolving each named `workspace:` dependency (`workspace:*`, `workspace:^`, `workspace:1.2.3`) once and reusing it across every project that declares it, instead of re-resolving it per project.
