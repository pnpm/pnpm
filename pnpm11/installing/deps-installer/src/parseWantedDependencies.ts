import type { Catalog } from '@pnpm/catalogs.types'
import type { WantedDependency } from '@pnpm/installing.deps-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { Dependencies } from '@pnpm/types'
import semver from 'semver'

export interface SelectorOutsideKeptRange {
  alias: string
  requested: string
  kept: string
}

export interface ParsedWantedDependencies {
  wantedDependencies: WantedDependency[]
  /**
   * The selectors that were dropped because the version they request doesn't satisfy the range
   * the manifest keeps. Only ever non-empty under `readonlyManifest`.
   */
  outsideKeptRange: SelectorOutsideKeptRange[]
}

export function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: {
    allowNew: boolean
    currentBareSpecifiers: Dependencies
    defaultTag: string
    dev: boolean
    devDependencies: Dependencies
    optional: boolean
    optionalDependencies: Dependencies
    overrides?: Record<string, string>
    updateWorkspaceDependencies?: boolean
    preferredSpecs?: Record<string, string>
    saveCatalogName?: string
    defaultCatalog?: Catalog
    /**
     * The manifest keeps its specifiers, so a requested version is applied only when it satisfies
     * the declared one — the lockfile importer entry has to keep satisfying its own specifier.
     */
    readonlyManifest?: boolean
  }
): ParsedWantedDependencies {
  const wantedDeps = rawWantedDependencies
    .map((rawWantedDependency) => {
      const parsed = parseWantedDependency(rawWantedDependency)
      const alias = parsed['alias']
      let bareSpecifier = parsed['bareSpecifier']

      if (!opts.allowNew && (!alias || !opts.currentBareSpecifiers[alias])) {
        return null
      }
      if (alias && opts.defaultCatalog?.[alias] && (
        (!opts.currentBareSpecifiers[alias] && bareSpecifier === undefined) ||
          opts.defaultCatalog[alias] === bareSpecifier ||
          opts.defaultCatalog[alias] === opts.currentBareSpecifiers[alias]
      )) {
        bareSpecifier = 'catalog:'
      }
      if (alias && opts.currentBareSpecifiers[alias]) {
        bareSpecifier ??= opts.currentBareSpecifiers[alias]
      }
      const result = {
        alias,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
        prevSpecifier: alias && opts.currentBareSpecifiers[alias],
        saveCatalogName: opts.saveCatalogName,
      } satisfies Partial<WantedDependency>
      if (bareSpecifier) {
        return {
          ...result,
          bareSpecifier,
        }
      }
      if (alias && opts.preferredSpecs?.[alias]) {
        return {
          ...result,
          bareSpecifier: opts.preferredSpecs[alias],
        }
      }
      if (alias && opts.overrides?.[alias]) {
        return {
          ...result,
          bareSpecifier: opts.overrides[alias],
        }
      }
      return {
        ...result,
        bareSpecifier: opts.defaultTag,
      }
    })
    .filter((wd) => wd !== null) as WantedDependency[]

  if (!opts.readonlyManifest) {
    return { wantedDependencies: wantedDeps, outsideKeptRange: [] }
  }
  const wantedDependencies: WantedDependency[] = []
  const outsideKeptRange: SelectorOutsideKeptRange[] = []
  for (const wantedDep of wantedDeps) {
    const { alias, bareSpecifier, prevSpecifier } = wantedDep
    if (!prevSpecifier || requestedVersionFitsKeptRange(bareSpecifier, prevSpecifier)) {
      wantedDependencies.push(wantedDep)
    } else {
      outsideKeptRange.push({ alias, requested: bareSpecifier, kept: prevSpecifier })
    }
  }
  return { wantedDependencies, outsideKeptRange }
}

/**
 * Whether the requested version satisfies the range the manifest keeps.
 *
 * Only a concrete version can be judged here. Matching a version against a range is exact;
 * deciding whether one *range* is contained by another is not — implementations disagree around
 * prerelease boundaries. So a requested range (`>=6`) or dist tag (`latest`) is left alone: it
 * has no version yet, and the reliable place to judge it is against the version resolution
 * settles on.
 */
function requestedVersionFitsKeptRange (requested: string, kept: string): boolean {
  if (semver.valid(requested) == null || semver.validRange(kept) == null) return true
  return semver.satisfies(requested, kept)
}
