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
   * The selectors that were dropped because the version they request reaches outside the range
   * the manifest keeps. Only ever non-empty under `keepManifestSpecifiers`.
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
     * The manifest won't be rewritten, so its specifiers stay authoritative and a requested
     * specifier may only narrow the one already declared.
     */
    keepManifestSpecifiers?: boolean
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

  if (!opts.keepManifestSpecifiers) {
    return { wantedDependencies: wantedDeps, outsideKeptRange: [] }
  }
  const wantedDependencies: WantedDependency[] = []
  const outsideKeptRange: SelectorOutsideKeptRange[] = []
  for (const wantedDep of wantedDeps) {
    const { alias, bareSpecifier, prevSpecifier } = wantedDep
    if (!prevSpecifier || staysWithinKeptRange(bareSpecifier, prevSpecifier)) {
      wantedDependencies.push(wantedDep)
    } else {
      outsideKeptRange.push({ alias, requested: bareSpecifier, kept: prevSpecifier })
    }
  }
  return { wantedDependencies, outsideKeptRange }
}

/**
 * Whether every version the requested specifier allows stays inside the range the manifest keeps.
 *
 * Overlapping is not enough: a requested `>=6` also allows versions above a kept `^6.0.0`, and
 * resolution would pick one of them. Specifiers that aren't semver ranges (dist tags,
 * `workspace:`, `catalog:`) can't be judged statically and are left to resolution.
 */
function staysWithinKeptRange (requested: string, kept: string): boolean {
  if (semver.validRange(requested) == null || semver.validRange(kept) == null) return true
  return semver.subset(requested, kept)
}
