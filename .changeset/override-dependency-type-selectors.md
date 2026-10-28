---
"pacquet": minor
---

`overrides` selectors now support `:prod`, `:optional`, and `:peer` pseudo-selectors that target a single dependency group. They also support `*` wildcard patterns on parent and target package names. An override with a non-range value, such as `npm:` or `file:`, no longer adds a peer dependency to `dependencies`. Removing an optional dependency with `"-"` now also removes the duplicate entry from `dependencies` [#5391](https://github.com/pnpm/pnpm/issues/5391).
