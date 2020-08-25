import matcher from '@pnpm/matcher'
import { SearchFunction } from 'dependencies-hierarchy'
import npa = require('@zkochan/npm-package-arg')
import semver = require('semver')

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
  if (!packageSelector.matchVersion) {
    return true
  }
  return !pkg.version.startsWith('link:') && packageSelector.matchVersion(pkg.version)
}

function parseSearchQuery (query: string) {
  const parsed = npa(query)
  if (parsed.raw === parsed.name) {
    return { matchName: matcher(parsed.name) }
  }
  if (parsed.type !== 'version' && parsed.type !== 'range') {
    throw new Error(`Invalid queryument - ${query}. List can search only by version or range`)
  }
  return {
    matchName: matcher(parsed.name),
    matchVersion: (version: string) => semver.satisfies(version, parsed.fetchSpec),
  }
}
