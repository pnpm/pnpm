---
"@pnpm/pnpr": patch
---

pnpr refuses SQL metadata and auth databases created by earlier alpha releases. Delete pnpr's SQL tables or point pnpr at empty databases before upgrading, including any local `tokens.db`. SQL token and migration timestamps now use milliseconds since the Unix epoch.
