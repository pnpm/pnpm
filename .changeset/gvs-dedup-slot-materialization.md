---
"pacquet": patch
---

Fixed installs under `enableGlobalVirtualStore` failing with `failed to remove existing directory ... prior to swap: Directory not empty` (or `No such file or directory`) when peer variants of an injected `file:` dependency hash to the same slot. The link pass now materializes each unique slot directory once instead of racing one force-mode import per peer variant against the same path.
