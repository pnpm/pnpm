---
"@pnpm/global.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm update --global --latest` failing with a 404 error when a globally installed package was not added from the registry by name. Packages installed from a local path (`link:`/`file:`), a git repository, a tarball URL, an `npm:` alias, or a named registry now keep their spec during a global update instead of being looked up by name in the default registry. See pnpm/pnpm#12854.
