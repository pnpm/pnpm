---
"@pnpm/store.cafs-types": major
"@pnpm/store.cafs": major
"@pnpm/worker": major
"@pnpm/store.controller": major
"@pnpm/store.commands": major
"@pnpm/deps.compliance.license-scanner": major
"@pnpm/modules-mounter.daemon": major
"pnpm": major
---

Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the hex digest instead of the full integrity string (`<algo>-<digest>`). Using hex format improves performance since file paths in the content-addressable store use hex representation, eliminating base64-to-hex conversion during path lookups.
