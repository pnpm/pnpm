import { createMatcher } from '@pnpm/matcher'
import npa from '@pnpm/npm-package-arg'
import { type FinderContext, type Finder } from '@pnpm/types'
import semver from 'semver'

export function createPackagesSearcher (queries: string[], finders?: Finder[]): Finder {
  const searchers: Finder[] = queries
    .map(parseSearchQuery)
    .map((packageSelector) => search.bind(null, packageSelector))
  return (pkg) => {
    if (searchers.length > 0 && searchers.every((search) => !search(pkg))) {
      return false
    }
    if (finders == null) return false
    const messages: string[] = []
    for (const finder of finders) {
      const result = finder(pkg)
      if (!result) return false
      if (typeof result === 'string') {
        messages.push(result)
      }
    }
    if (messages.length) return messages.join('\n')
    return true
  }
}

type MatchFunction = (entry: string) => boolean

function search (
  packageSelector: {
    matchName: MatchFunction
    matchVersion?: MatchFunction
  },
  { name, version }: FinderContext
): boolean {
  if (!packageSelector.matchName(name)) {
    return false
  }
  if (packageSelector.matchVersion == null) {
    return true
  }
  return !version.startsWith('link:') && packageSelector.matchVersion(version)
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
