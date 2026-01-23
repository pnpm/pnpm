---
"@pnpm/cafs-types": major
"@pnpm/store.cafs": major
"@pnpm/worker": major
"@pnpm/package-store": major
"@pnpm/plugin-commands-store-inspecting": major
"@pnpm/license-scanner": major
"@pnpm/modules-mounter": major
"pnpm": major
---

Optimized index file format to store the hash algorithm once per file instead of repeating it for every file entry. Each file entry now stores only the base64 digest instead of the full integrity string (`<algo>-<digest>`). This reduces index file size and avoids redundant string construction when verifying integrity.
