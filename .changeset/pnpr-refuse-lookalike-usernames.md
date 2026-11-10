---
"@pnpm/pnpr": patch
---

Account creation and renaming now refuse usernames that differ from an existing account only by letter case or accents. pnpr also refuses usernames that mix Unicode scripts or contain restricted Unicode characters.
