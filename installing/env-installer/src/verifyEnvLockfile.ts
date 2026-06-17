import { assertValidDependencyAliases } from '@pnpm/installing.deps-resolver'
import type { EnvLockfile } from '@pnpm/lockfile.fs'

import { assertValidConfigDepVersion } from './assertValidConfigDepVersion.js'

// Offline structural gate for the env lockfile, mirroring the alias/shape
// checks `verifyLockfileResolutions` runs over the main lockfile. Config
// dependency and optional-subdependency names and versions become store path
// segments, so reject any that isn't a valid npm name / exact semver before a
// path is built from them.
export function verifyEnvLockfile (envLockfile: EnvLockfile): void {
  const configDeps = envLockfile.importers['.']?.configDependencies
  assertValidDependencyAliases(configDeps, 'The configDependencies in pnpm-lock.yaml')
  if (configDeps == null) return
  for (const [name, { version }] of Object.entries(configDeps)) {
    assertValidConfigDepVersion(name, version)
    const optionalDeps = envLockfile.snapshots[`${name}@${version}`]?.optionalDependencies
    if (optionalDeps == null) continue
    assertValidDependencyAliases(optionalDeps, `The optionalDependencies of config dependency "${name}" in pnpm-lock.yaml`)
    for (const [subdepName, subdepVersion] of Object.entries(optionalDeps)) {
      assertValidConfigDepVersion(subdepName, subdepVersion)
    }
  }
}
