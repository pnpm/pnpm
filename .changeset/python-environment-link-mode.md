---
"pacquet": minor
---

Python environments now use `packageImportMethod` to import wheel files from the store. Use `clone-or-copy` for copy-on-write clones with a copy fallback, or `copy` for independent files. Hardlinked files share writes with the store and other environments.

Isolated Python build environments keep backend writes private with copy-on-write clones or copies.
