---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Sped up resolution and reduced memory use against registries that do not implement npm's abbreviated metadata format (for example, Azure DevOps Artifacts). Such registries ignore the `application/vnd.npm.install-v1+json` request and return the full package document — including per-version fields the installer never reads (`scripts`, `exports`, `devDependencies`, custom `package.json` fields, etc.). pnpm now detects this at the point of fetching (via the response `Content-Type`) and normalizes the document down to the abbreviated field set before it enters the cache, so subsequent resolutions read and parse a much smaller document. Registries that honor the abbreviated header (such as the npm registry) are detected as already-abbreviated and pay no extra cost. On a large workspace whose feed serves full documents, this roughly halved `pnpm dedupe --offline` time and cut peak memory, with no change to resolution output.
