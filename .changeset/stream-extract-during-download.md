---
"pacquet": patch
---

Large tarballs are now verified and extracted while they download instead of after: their content hash and content-addressable-store writes ride along with the arriving body. The biggest packages are the ones whose downloads finish last, so their extraction used to run after the last byte of the install — pure added wall time at the end of every cold install.
