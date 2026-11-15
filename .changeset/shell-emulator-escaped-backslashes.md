---
"pacquet": patch
---

`shellEmulator` now handles escaped backslashes (`\\`) and unquoted escaped single quotes (`\'`) according to POSIX quoting rules. Previously, scripts containing `"a\\"` or `\'` failed to parse or misquoted later `${...}` parameter expansions.
