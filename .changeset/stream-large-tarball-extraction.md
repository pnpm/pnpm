---
"pacquet": patch
---

Reduced peak memory usage when installing large packages. A tarball whose compressed size is at least 16 MiB, or whose registry-reported unpacked size is at least 64 MiB, is now extracted by streaming the decompression directly into the content-addressable store instead of materializing the whole decompressed archive in memory, and its large files are hashed and written to the store incrementally.
