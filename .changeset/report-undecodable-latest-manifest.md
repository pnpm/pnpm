---
"pacquet": patch
---

When `dist-tags.latest` names a version whose manifest pnpm cannot read, the error now names that version and the field it could not decode, instead of reporting the tag as empty.
