import { PnpmError } from '@pnpm/error'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { type Catalogs } from '@pnpm/catalogs.types'

const DELIMITER_REGEX = /[^ |@]>/

export interface VersionOverride {
  selector: string
  parentPkg?: PackageSelector
  targetPkg: PackageSelector
  newBareSpecifier: string
}

export interface PackageSelector {
  name: string
  bareSpecifier?: string
}

export function parseOverrides (
  overrides: Record<string, string>,
  catalogs?: Catalogs
): VersionOverride[] {
  const _resolveFromCatalog = resolveFromCatalog.bind(null, catalogs ?? {})
  return Object.entries(overrides)
    .map(([selector, newBareSpecifier]) => {
      const result = parsePkgAndParentSelector(selector)
      const resolvedCatalog = matchCatalogResolveResult(_resolveFromCatalog({
        bareSpecifier: newBareSpecifier,
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
        newBareSpecifier: resolvedCatalog ?? newBareSpecifier,
        ...result,
      }
    })
}

export function parsePkgAndParentSelector (selector: string): Pick<VersionOverride, 'parentPkg' | 'targetPkg'> {
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
    bareSpecifier: wantedDep.bareSpecifier,
  }
}
