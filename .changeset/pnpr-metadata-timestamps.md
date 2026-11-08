---
"@pnpm/pnpr": patch
---

pnpr refuses metadata databases created by earlier alpha releases. Reset the metadata database before upgrading. Publication retention and archive reclamation now use the same clock [task](https://github.com/zkochan/pnpm-tasks/issues/33).
