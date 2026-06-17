import { PnpmError } from '@pnpm/error'
import semver from 'semver'

// A config dependency's version comes from the committed lockfile/manifest and
// becomes a store path segment (`<name>/<version>/<hash>`). Config deps resolve
// to exact versions, so reject anything that isn't valid semver — a
// traversal-shaped version would otherwise escape the store root.
export function assertValidConfigDepVersion (name: string, version: string): void {
  if (semver.valid(version) == null) {
    throw new PnpmError(
      'INVALID_CONFIG_DEP_VERSION',
      `The config dependency "${name}" has an invalid version "${version}"`,
      { hint: 'A config dependency version must be an exact semver version.' }
    )
  }
}
