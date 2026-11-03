---
"@pnpm/pnpr": patch
---

pnpr refuses a SQL metadata database of another schema version before it creates any table in it. The error names the version pnpr expects and what the database holds, and says to wipe pnpr's metadata tables, which pnpr does not migrate before 1.0.
