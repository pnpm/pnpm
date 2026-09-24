---
"@pnpm/deps.path": patch
"pnpm": patch
"pacquet": patch
---

pnpm now escapes trailing dots and spaces in `node_modules/.pnpm` directory names. Windows strips these characters, so a dependency such as `"parent-pkg": "file:../"` created a directory that could not be deleted or failed to install [#8101](https://github.com/pnpm/pnpm/issues/8101).
