---
"@pnpm/pnpr": minor
---

pnpr now runs numbered schema migrations for its metadata and auth tables. Each configured SQL database records its applied steps in its own `schema_migrations` table, starting at version `1`. pnpr refuses to start on a pre-1.0 database that has no `schema_migrations` table. Back up each affected metadata or token database before wiping its pnpr tables. Wiping the tables in a database deletes the hosted package metadata or SQL-backed accounts and tokens stored there. Recreated SQL accounts receive new IDs. Republish packages, recreate affected accounts and tokens, update account ID references in `auth.admins`, access lists, and team memberships, reconcile the SCIM directory with the identity provider and any retained accounts, and restore managed access settings before accepting traffic.
