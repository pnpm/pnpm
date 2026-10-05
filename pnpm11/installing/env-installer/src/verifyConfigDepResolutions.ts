import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolutionVerifier } from '@pnpm/resolving.npm-resolver'
import type { ConfigDependencies } from '@pnpm/types'

import { parseIntegrity } from './parseIntegrity.js'
import type { ResolveAndInstallConfigDepsOpts } from './resolveAndInstallConfigDeps.js'
import { verifyEnvLockfile } from './verifyEnvLockfile.js'

export function createConfigDepsVerifier (configDeps: ConfigDependencies, opts: ResolveAndInstallConfigDepsOpts): (env: EnvLockfile) => Promise<void> {
  const verifier = createNpmResolutionVerifier(configVerifierOptions(opts))
  const pins = configDependencyPins(configDeps)
  return async (env) => {
    verifyEnvLockfile(env)
    assertConfiguredPins(env, pins)
    const keys = Array.from(configDependencyKeys(env)).filter((key) => !pins.has(key))
    await Promise.all(keys.map((key) => verifyConfigDependency(key, env.packages[key], verifier)))
  }
}

function configDependencyPins (configDeps: ConfigDependencies): Map<string, string> {
  const pins = new Map<string, string>()
  for (const [name, value] of Object.entries(configDeps)) {
    const spec = typeof value === 'object' ? value.integrity : value
    if (!spec.includes('+')) continue
    const { version, integrity } = parseIntegrity(name, spec)
    pins.set(`${name}@${version}`, integrity)
  }
  return pins
}

function assertConfiguredIntegrity (key: string, resolution: EnvLockfile['packages'][string]['resolution'], pin: string): void {
  if (!('integrity' in resolution) || resolution.integrity !== pin) {
    throw new PnpmError('BAD_CONFIG_DEP', `Configuration dependency "${key}" does not match its configured integrity`)
  }
}

function assertConfiguredPins (env: EnvLockfile, pins: Map<string, string>): void {
  for (const [key, pin] of pins) {
    const separator = key.lastIndexOf('@')
    const name = key.slice(0, separator)
    const version = key.slice(separator + 1)
    const metadata = env.packages[key]
    if (env.importers['.'].configDependencies[name]?.version !== version || !metadata) {
      throw new PnpmError('BAD_CONFIG_DEP', `Configuration dependency "${key}" does not match its configured integrity`)
    }
    assertConfiguredIntegrity(key, metadata.resolution, pin)
  }
}

function configDependencyKeys (env: EnvLockfile): Set<string> {
  const keys = new Set<string>()
  for (const [name, dependency] of Object.entries(env.importers['.'].configDependencies ?? {})) {
    const key = `${name}@${dependency.version}`
    keys.add(key)
    for (const [subdepName, subdepVersion] of Object.entries(env.snapshots[key]?.optionalDependencies ?? {})) {
      keys.add(`${subdepName}@${subdepVersion}`)
    }
  }
  return keys
}

function configVerifierOptions (opts: ResolveAndInstallConfigDepsOpts): Parameters<typeof createNpmResolutionVerifier>[0] {
  return {
    ...opts,
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime ?? opts.ignoreMissingTimeField,
    getAuthHeaderValueByURI: createGetAuthHeaderByURI(opts.configByUri ?? {}),
    fetchOpts: {
      fetch: createFetchFromRegistry(opts),
      retry: opts.retry ?? {},
      timeout: opts.timeout ?? 60000,
      fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs ?? 10000,
    },
  }
}

async function verifyConfigDependency (
  key: string,
  metadata: EnvLockfile['packages'][string] | undefined,
  verifier: ReturnType<typeof createNpmResolutionVerifier>
): Promise<void> {
  if (!metadata) throw new PnpmError('BAD_CONFIG_DEP', `Missing configuration dependency "${key}"`)
  const entry = nameVerFromPkgSnapshot(key, { resolution: metadata.resolution })
  if (!entry.name || !entry.version) throw new PnpmError('BAD_CONFIG_DEP', `Invalid configuration dependency "${key}"`)
  const result = await verifier.verify(metadata.resolution as Parameters<typeof verifier.verify>[0], entry)
  if (!result.ok) throw new PnpmError('BAD_CONFIG_DEP', `Configuration dependency "${key}" ${result.reason}`)
}
