---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

Don't print "Installing config dependencies..." when config dependencies are already installed and nothing needs to be fetched, re-linked, or removed.
