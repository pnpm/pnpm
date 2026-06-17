import { PnpmError } from '@pnpm/error'
import semver from 'semver'

// A config dependency's version is read from attacker-controlled input (the
// env lockfile or the legacy inline-integrity manifest format) and becomes a
// path segment of the global virtual store path (`<name>/<version>/<hash>`), so
// a traversal-shaped version like `../../../PWNED` would let a malicious
// repository write outside the intended roots during install. Config deps
// resolve to exact versions, so anything that isn't a valid semver version is
// rejected before any path is built or any lockfile is written.
export function assertValidConfigDepVersion (name: string, version: string): void {
  if (semver.valid(version) == null) {
    throw new PnpmError(
      'INVALID_CONFIG_DEP_VERSION',
      `The config dependency "${name}" has an invalid version "${version}"`,
      { hint: 'A config dependency version must be an exact semver version.' }
    )
  }
}
