import type { CatalogResolutionFound, CatalogResolutionMisconfiguration, CatalogResolutionResult, CatalogResolutionUnused } from './resolveFromCatalog.js'

export interface CatalogResultMatcher<Outcome> {
  readonly found: (found: CatalogResolutionFound) => Outcome
  readonly misconfiguration: (misconfiguration: CatalogResolutionMisconfiguration) => Outcome
  readonly unused: (unused: CatalogResolutionUnused) => Outcome
}

export function matchCatalogResolveResult<Outcome> (
  result: CatalogResolutionResult,
  matcher: CatalogResultMatcher<Outcome>
): Outcome {
  switch (result.type) {
    case 'found': return matcher.found(result)
    case 'misconfiguration': return matcher.misconfiguration(result)
    case 'unused': return matcher.unused(result)
  }
}
