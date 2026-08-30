---
"pacquet": patch
---

Fixed concurrent installs sharing a Global Virtual Store sometimes failing while another installer replaces the same command shim between writing it and making it executable.
