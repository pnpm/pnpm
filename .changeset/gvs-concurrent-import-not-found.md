---
"pacquet": patch
---

Fixed concurrent installs that share a global virtual store on macOS failing with "failed to import ... No such file or directory" while another install was populating the same package [#14560](https://github.com/pnpm/pnpm/issues/14560).
