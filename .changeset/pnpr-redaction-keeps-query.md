---
"@pnpm/pnpr": patch
---

pnpr records a URL's query exactly as it was written when it redacts the credentials in an error message. It used to re-encode the query, rewriting `?options=a%20b` as `?options=a+b`.
