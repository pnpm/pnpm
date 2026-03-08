import type { PreferredVersions } from '@pnpm/resolver-base'
import type { WantedDependency } from './getWantedDependencies.js'
import { unwrapPackageName } from './unwrapPackageName.js'

/**
 * Create a PreferredVersions object with a specific exact version.
 */
export function getExactSinglePreferredVersions (wantedDependency: WantedDependency, version: string): PreferredVersions {
  const { pkgName } = unwrapPackageName(wantedDependency.alias, wantedDependency.bareSpecifier)
  return {
    [pkgName]: { [version]: 'version' },
  }
}
