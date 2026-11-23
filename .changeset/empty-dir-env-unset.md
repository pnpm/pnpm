---
"pnpm": patch
---

pnpm now treats an empty `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, or `PNPM_HOME` as unset and uses the default directory. An empty value made pnpm read and write its global config, auth file, and cache in a `pnpm` directory under the current working directory.
