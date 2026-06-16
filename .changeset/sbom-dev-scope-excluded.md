---
"@pnpm/deps.compliance.sbom": minor
"pnpm": minor
---

`pnpm sbom` now marks components reachable only through `devDependencies` with CycloneDX `scope: "excluded"` and the `cdx:npm:package:development` property. The `excluded` scope documents "component usage for test and other non-runtime purposes", which matches the semantics of a devDependency; the property is the CycloneDX npm-taxonomy marker emitted by `@cyclonedx/cyclonedx-npm`, so both modern (scope) and existing (property) consumers are covered. Components reachable at runtime (including installed `optionalDependencies`) omit `scope` and default to `required`.
