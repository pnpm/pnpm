import { assertValidDependencyAliases } from '@pnpm/installing.deps-resolver'
import type { EnvLockfile } from '@pnpm/lockfile.fs'

import { assertValidConfigDepVersion } from './assertValidConfigDepVersion.js'

// Offline structural gate for the env lockfile (the config-dependency YAML
// document), mirroring the always-on alias/shape checks
// `verifyLockfileResolutions` runs over the main lockfile. Every config
// dependency and optional-subdependency name and version becomes a store path
// segment (`node_modules/.pnpm-config/<name>`, `<name>/<version>/<hash>`), so a
// committed lockfile with a traversal-shaped name or version would escape the
// install roots. Run it on the in-memory env lockfile before any path is built
// or any lockfile is written.
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
