---
"@pnpm/cafs": major
---

Instead of creating a separate subdir for executables in the content-addressable storage, use the directory where all the files are stored but suffix the executable files with `-exec`. Also suffix the package index files with `-index.json`.
