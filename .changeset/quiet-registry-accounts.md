---
"@pnpm/pnpr": patch
---

User lookups now require authentication. Team names and rosters are visible only to registry team members and administrators. Passwords must contain at least 8 characters.

New accounts cannot use usernames that look like an existing username. SQL auth databases from earlier releases must be recreated.
