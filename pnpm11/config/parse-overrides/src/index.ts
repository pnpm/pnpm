import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import semver from 'semver'

export { isIntersectingRange } from './isIntersectingRange.js'

const DELIMITER_REGEX = /[^ |@]>/

export interface VersionOverride {
  selector: string
  parentPkg?: PackageSelector
  targetPkg: PackageSelector
  newBareSpecifier: string
  /**
   * True for empty-range selectors (`"pkg@"`): a convergence override. It
   * applies to a dependency edge only when `newBareSpecifier` (always an
   * exact version) satisfies the edge's declared range, so applying it can
   * never violate a consumer's range.
   */
  converge?: boolean
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
      const override = {
        selector,
        newBareSpecifier: resolvedCatalog ?? newBareSpecifier,
        ...result,
      }
      return markConvergeOverride(override)
    })
}

function markConvergeOverride (override: VersionOverride): VersionOverride {
  const emptyRangeInParentChildSelector = override.parentPkg != null &&
    (override.parentPkg.bareSpecifier === '' || override.targetPkg.bareSpecifier === '')
  if (emptyRangeInParentChildSelector) {
    throw new PnpmError('INVALID_CONVERGENCE_OVERRIDE', `Cannot use an empty range in the "${override.selector}" selector: convergence overrides ("pkg@") cannot be combined with parent>child selectors`)
  }
  if (override.targetPkg.bareSpecifier !== '') return override
  if (semver.valid(override.newBareSpecifier) == null) {
    throw new PnpmError('INVALID_CONVERGENCE_OVERRIDE', `The value of the convergence override "${override.selector}" must be an exact version, but got "${override.newBareSpecifier}"`)
  }
  override.converge = true
  return override
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
