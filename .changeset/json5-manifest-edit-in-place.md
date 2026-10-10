---
"pacquet": minor
---

When pnpm updates a `package.json5`, it now changes only the values that changed. Comments, quotes, number formats, and layout elsewhere in the file stay as written. New keys and strings use the quote style of the file.
