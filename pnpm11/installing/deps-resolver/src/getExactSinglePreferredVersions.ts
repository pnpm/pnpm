import type { PreferredVersions } from '@pnpm/resolving.resolver-base'

import type { ManifestWantedDependency } from './getWantedDependencies.js'
import { unwrapPackageName } from './unwrapPackageName.js'

/**
 * Create a PreferredVersions object with a specific exact version.
 */
export function getExactSinglePreferredVersions (wantedDependency: ManifestWantedDependency, version: string): PreferredVersions {
  const { pkgName } = unwrapPackageName(wantedDependency.alias, wantedDependency.bareSpecifier)
  return {
    [pkgName]: { [version]: 'version' },
  }
}
