---
"pacquet": patch
---

An invalid entry in a Python registry's `packages` list now names the patterns a Python registry accepts: an exact name, a prefix such as `company-*`, or `**`. The error used to suggest `@scope/*`, which a Python registry rejects. It also quotes the entry as written. A rejected prefix such as `-a*` used to appear as `-ax`.
