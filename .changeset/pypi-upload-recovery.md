---
"@pnpm/pnpr": patch
---

Python uploads now reject unreadable archives. Uploads also reject metadata that names a different project or version. Publishers can change the yank status of individual files. Administrators can delete files from the index and disable their downloads. Deletion retains stored archive bytes.
