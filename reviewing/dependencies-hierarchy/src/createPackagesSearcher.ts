import { createMatcher } from '@pnpm/matcher'
import npa from '@pnpm/npm-package-arg'
import { type SearchFunction } from './types'
import semver from 'semver'

export function createPackagesSearcher (queries: string[]): SearchFunction {
  const searchers: SearchFunction[] = queries
    .map(parseSearchQuery)
    .map((packageSelector) => search.bind(null, packageSelector))
  return (pkg) => searchers.some((search) => search(pkg))
}

type MatchFunction = (entry: string) => boolean

function search (
  packageSelector: {
    matchName: MatchFunction
    matchVersion?: MatchFunction
  },
  pkg: { name: string, version: string }
): boolean {
  if (!packageSelector.matchName(pkg.name)) {
    return false
  }
  if (packageSelector.matchVersion == null) {
    return true
  }
  return !pkg.version.startsWith('link:') && packageSelector.matchVersion(pkg.version)
}

interface ParsedSearchQuery {
  matchName: (name: string) => boolean
  matchVersion?: (version: string) => boolean
}

function parseSearchQuery (query: string): ParsedSearchQuery {
  const parsed = npa(query)
  if (parsed.raw === parsed.name) {
    return { matchName: createMatcher(parsed.name) }
  }
  if (parsed.type !== 'version' && parsed.type !== 'range') {
    throw new Error(`Invalid query - ${query}. List can search only by version or range`)
  }
  return {
    matchName: createMatcher(parsed.name),
    matchVersion: (version: string) => semver.satisfies(version, parsed.fetchSpec),
  }
}
