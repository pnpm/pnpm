---
"pnpm": patch
"@pnpm/pnpr": patch
---

pnpm now reads more malformed package tarballs the way npm does, and installs the same files from them. Among them are a tar that continues into a second gzip member, a header whose checksum runs into its type field, and a header after a single zero block. pnpm also ignores data after the end-of-archive marker, as npm does.
