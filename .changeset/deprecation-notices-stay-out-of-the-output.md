---
"@pnpm/core-loggers": major
"@pnpm/resolving.resolver-base": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/store.controller-types": minor
"@pnpm/cli.default-reporter": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.package-requester": patch
"@pnpm/text.sanitize": patch
"pnpm": patch
"pacquet": patch
---

Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

A deprecation warning now names the newest version of the package that is not deprecated, and says when reaching it means widening the range you declared:

```
WARN  deprecated foo@1.0.0. 2.3.1 is not deprecated, outside the range you declared.
```

pnpm works this out from the metadata it already fetched, so it costs no extra request. An install that reuses the lockfile without fetching metadata names no version.

pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.
