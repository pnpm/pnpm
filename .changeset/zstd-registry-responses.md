---
"pacquet": minor
---

pnpm now accepts zstd-compressed responses. Registries that serve zstd send package metadata in fewer bytes than with gzip, most of all the full metadata that settings such as `minimumReleaseAge` and `trustPolicy` read.
