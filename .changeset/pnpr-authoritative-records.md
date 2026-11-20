---
"@pnpm/pnpr": patch
---

pnpr now keeps package ownership after unpublish, team rosters, rule overrides, SCIM users, and staged-publish metadata in its metadata database. An unpublish records ownership and removed versions atomically. This changes the metadata schema and requires resetting existing alpha deployments.
