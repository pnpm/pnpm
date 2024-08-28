import { PnpmError } from '@pnpm/error'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'

const DELIMITER_REGEX = /[^ |@]>/

export interface VersionOverride {
  selector: string
  parentPkg?: PackageSelector
  targetPkg: PackageSelector
  newPref: string
  refTarget?: string
}

export interface PackageSelector {
  name: string
  pref?: string
}

export interface ParseOverridesOptions {
  overridesRefMap?: Record<string, string | undefined>
  catalogs?: Catalogs
}

export function parseOverrides (
  overrides: Record<string, string>,
  opts?: ParseOverridesOptions
): VersionOverride[] {
  const _resolveFromCatalog = resolveFromCatalog.bind(null, opts?.catalogs ?? {})
  return Object.entries(overrides)
    .map(([selector, newPref]) => {
      const result = parsePkgAndParentSelector(selector)
      const resolvedCatalog = matchCatalogResolveResult(_resolveFromCatalog({
        pref: newPref,
        alias: result.targetPkg.name,
      }), {
        found: ({ resolution }) => resolution.specifier,
        unused: () => undefined,
        misconfiguration: ({ error }) => {
          throw new PnpmError('CATALOG_IN_OVERRIDES', `Could not resolve a catalog in the overrides: ${error.message}`)
        },
      })
      return {
        selector,
        newPref: resolvedCatalog ?? newPref,
        refTarget: opts?.overridesRefMap?.[selector],
        ...result,
      }
    })
}

function parsePkgAndParentSelector (selector: string): Pick<VersionOverride, 'parentPkg' | 'targetPkg'> {
  let delimiterIndex = selector.search(DELIMITER_REGEX)
  if (delimiterIndex !== -1) {
    delimiterIndex++
    const parentSelector = selector.substring(0, delimiterIndex)
    const childSelector = selector.substring(delimiterIndex + 1)
    return {
      parentPkg: parsePkgSelector(parentSelector),
      targetPkg: parsePkgSelector(childSelector),
    }
  }
  return {
    targetPkg: parsePkgSelector(selector),
  }
}

function parsePkgSelector (selector: string): PackageSelector {
  const wantedDep = parseWantedDependency(selector)
  if (!wantedDep.alias) {
    throw new PnpmError('INVALID_SELECTOR', `Cannot parse the "${selector}" selector`)
  }
  return {
    name: wantedDep.alias,
    pref: wantedDep.pref,
  }
}
