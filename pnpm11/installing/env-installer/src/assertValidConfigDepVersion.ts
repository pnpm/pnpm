import { PnpmError } from '@pnpm/error'
import semver from 'semver'

// A config-dep version becomes a store path segment (`<name>/<version>/<hash>`),
// so reject non-semver values to keep a traversal-shaped version from escaping
// the store root.
export function assertValidConfigDepVersion (name: string, version: string): void {
  if (semver.valid(version) == null) {
    throw new PnpmError(
      'INVALID_CONFIG_DEP_VERSION',
      `The config dependency "${name}" has an invalid version "${version}"`,
      { hint: 'A config dependency version must be an exact semver version.' }
    )
  }
}
