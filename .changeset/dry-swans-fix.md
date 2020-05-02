---
"@pnpm/tarball-fetcher": major
---

There is no reason to keep the tarballs on the disk.
All the files are unpacked and their checksums are stored.
So the tarball is only used if someone modifies the content of
the unpacked package. In that rare case, it is fine if we
redownload the tarball from the registry.
