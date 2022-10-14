import { createMatcher } from '@pnpm/matcher'
import npa from '@pnpm/npm-package-arg'
import { SearchFunction } from 'dependencies-hierarchy'
import semver from 'semver'

export default function createPatternSearcher (queries: string[]) {
  const searchers: SearchFunction[] = queries
    .map(parseSearchQuery)
    .map((packageSelector) => search.bind(null, packageSelector))
  return (pkg: { name: string, version: string }) => searchers.some((search) => search(pkg))
}

type MatchFunction = (entry: string) => boolean

function search (
  packageSelector: {
    matchName: MatchFunction
    matchVersion?: MatchFunction
  },
  pkg: { name: string, version: string }
) {
  if (!packageSelector.matchName(pkg.name)) {
    return false
  }
  if (packageSelector.matchVersion == null) {
    return true
  }
  return !pkg.version.startsWith('link:') && packageSelector.matchVersion(pkg.version)
}

function parseSearchQuery (query: string) {
  const parsed = npa(query)
  if (parsed.raw === parsed.name) {
    return { matchName: createMatcher(parsed.name) }
  }
  if (parsed.type !== 'version' && parsed.type !== 'range') {
    throw new Error(`Invalid queryment - ${query}. List can search only by version or range`)
  }
  return {
    matchName: createMatcher(parsed.name),
    matchVersion: (version: string) => semver.satisfies(version, parsed.fetchSpec),
  }
}
