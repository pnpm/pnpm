---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Sped up resolution and reduced memory use against registries that ignore npm's abbreviated metadata format and always return the full package document (for example, Azure DevOps Artifacts). pnpm now strips such documents down to the abbreviated field set before caching them. Resolution output is unchanged, and registries that honor the abbreviated format (such as the npm registry) pay no extra cost.
