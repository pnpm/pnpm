import type { Catalog } from '@pnpm/catalogs.types'
import type { WantedDependency } from '@pnpm/installing.deps-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { Dependencies } from '@pnpm/types'
import semver from 'semver'

export interface KeptRangeConflict {
  alias: string
  requested: string
  kept: string
}

export interface ParsedWantedDependencies {
  wantedDependencies: WantedDependency[]
  /**
   * The selectors dropped because the version they request doesn't satisfy the range the manifest
   * keeps. Those dependencies are left untouched. Only ever non-empty under `readonlyManifest`.
   */
  outsideKeptRange: KeptRangeConflict[]
  /**
   * The selectors resolution can't be trusted to honor — a range or a dist tag, which only names a
   * version once resolution has run — so the specifier the manifest keeps was used instead. Only
   * ever non-empty under `readonlyManifest`.
   */
  supersededByKeptRange: KeptRangeConflict[]
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
    return { wantedDependencies: wantedDeps, outsideKeptRange: [], supersededByKeptRange: [] }
  }
  const wantedDependencies: WantedDependency[] = []
  const outsideKeptRange: KeptRangeConflict[] = []
  const supersededByKeptRange: KeptRangeConflict[] = []
  for (const wantedDep of wantedDeps) {
    const { alias, bareSpecifier, prevSpecifier } = wantedDep
    if (!prevSpecifier || bareSpecifier === prevSpecifier) {
      wantedDependencies.push(wantedDep)
    } else if (semver.valid(bareSpecifier) != null && semver.validRange(prevSpecifier) != null) {
      // Both sides are concrete enough to judge now: matching a version against a range is exact.
      if (semver.satisfies(bareSpecifier, prevSpecifier)) {
        wantedDependencies.push(wantedDep)
      } else {
        outsideKeptRange.push({ alias, requested: bareSpecifier, kept: prevSpecifier })
      }
    } else {
      // A range, a dist tag, or a kept specifier that isn't a semver range. Nothing here names a
      // version yet, and asking whether one range contains another is not answered consistently
      // across semver implementations — so resolve what the manifest declares, which is the
      // specifier the importer entry will record.
      supersededByKeptRange.push({ alias, requested: bareSpecifier, kept: prevSpecifier })
      wantedDependencies.push({ ...wantedDep, bareSpecifier: prevSpecifier })
    }
  }
  return { wantedDependencies, outsideKeptRange, supersededByKeptRange }
}
