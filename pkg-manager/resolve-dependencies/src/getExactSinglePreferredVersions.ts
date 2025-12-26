import { type PreferredVersions } from '@pnpm/resolver-base'
import { getRealNameAndSpec } from './getRealNameAndSpec.js'
import { type WantedDependency } from './getWantedDependencies.js'

/**
 * Create a PreferredVersions object with a specific exact version.
 */
export function getExactSinglePreferredVersions (wantedDependency: WantedDependency, version: string): PreferredVersions {
  const { pkgName } = getRealNameAndSpec(wantedDependency.alias, wantedDependency.bareSpecifier)
  return {
    [pkgName]: { [version]: 'version' },
  }
}
