import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { installConfigDeps } from '@pnpm/installing.env-installer'
import { createEnvLockfile, type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { loadJsonFileSync } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

function makeEnvLockfile (deps: Record<string, { version: string, integrity: string }>): EnvLockfile {
  const lockfile = createEnvLockfile()
  for (const [name, { version, integrity }] of Object.entries(deps)) {
    const pkgKey = `${name}@${version}`
    lockfile.importers['.'].configDependencies[name] = { specifier: version, version }
    lockfile.packages[pkgKey] = { resolution: { integrity } }
    lockfile.snapshots[pkgKey] = {}
  }
  return lockfile
}

test('configuration dependency is installed from env lockfile', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const lockfile = makeEnvLockfile({
    '@pnpm.e2e/foo': { version: '100.0.0', integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  })
  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
    // The local path should be a symlink to the global virtual store
    expect(fs.lstatSync('node_modules/.pnpm-config/@pnpm.e2e/foo').isSymbolicLink()).toBe(true)
  }

  // Dependency is updated
  const lockfile2 = makeEnvLockfile({
    '@pnpm.e2e/foo': { version: '100.1.0', integrity: getIntegrity('@pnpm.e2e/foo', '100.1.0') },
  })

  await installConfigDeps(lockfile2, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.1.0')
  }

  // Dependency is removed
  const lockfile3 = createEnvLockfile()

  await installConfigDeps(lockfile3, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')).toBeFalsy()
})

test('optional subdep matching the current platform is installed and symlinked next to parent', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/support-different-architectures'
  const parentVersion = '1.0.0'
  // Build a subdep entry that matches the current platform exactly.
  const matchingSubdepName = `@pnpm.e2e/only-${process.platform}-${process.arch}`
  const matchingSubdepVersion = '1.0.0'
  const incompatibleSubdepName = '@pnpm.e2e/only-darwin-arm64'

  const lockfile = createEnvLockfile()
  const parentKey = `${parentName}@${parentVersion}`
  lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
  lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
  lockfile.snapshots[parentKey] = {
    optionalDependencies: {
      [matchingSubdepName]: matchingSubdepVersion,
      // Force-include a darwin-arm64 entry to verify cross-platform skip logic.
      // If the test runs on darwin-arm64, this is the matching one and is added
      // twice (no-op due to map semantics), so we only assert the unrelated
      // platform skip when running on other architectures.
      ...(matchingSubdepName !== incompatibleSubdepName
        ? { [incompatibleSubdepName]: '1.0.0' }
        : {}),
    },
  }
  lockfile.packages[`${matchingSubdepName}@${matchingSubdepVersion}`] = {
    resolution: { integrity: getIntegrity(matchingSubdepName, matchingSubdepVersion) },
    os: [process.platform],
    cpu: [process.arch],
  }
  if (matchingSubdepName !== incompatibleSubdepName) {
    lockfile.packages[`${incompatibleSubdepName}@1.0.0`] = {
      resolution: { integrity: getIntegrity(incompatibleSubdepName, '1.0.0') },
      os: ['darwin'],
      cpu: ['arm64'],
    }
  }

  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  // Parent is symlinked into .pnpm-config
  expect(fs.existsSync(`node_modules/.pnpm-config/${parentName}/package.json`)).toBe(true)

  // The matching subdep is reachable from the parent's location via Node-style
  // module resolution (walk up to siblings in node_modules).
  const parentRealPath = fs.realpathSync(`node_modules/.pnpm-config/${parentName}`)
  const subdepSiblingPath = `${parentRealPath}/../${matchingSubdepName}`
  expect(fs.existsSync(`${subdepSiblingPath}/package.json`)).toBe(true)
  const subdepManifest = loadJsonFileSync<{ name: string }>(`${subdepSiblingPath}/package.json`)
  expect(subdepManifest.name).toBe(matchingSubdepName)

  // The non-matching subdep is NOT linked next to the parent
  if (matchingSubdepName !== incompatibleSubdepName) {
    expect(fs.existsSync(`${parentRealPath}/../${incompatibleSubdepName}`)).toBe(false)
  }
})

test('installation fails if the checksum of the config dependency is invalid', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore({
    clientOptions: {
      retry: {
        retries: 0,
      },
    },
  })

  const lockfile = makeEnvLockfile({
    '@pnpm.e2e/foo': {
      version: '100.0.0',
      integrity: 'sha512-00000000000000000000000000000000000000000000000000000000000000000000000000000000000000==',
    },
  })
  await expect(installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })).rejects.toThrow('Got unexpected checksum for')
})

test('migration: installs from old inline integrity format and creates env lockfile', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  // Old format: ConfigDependencies with inline integrity
  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }
  await installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
  }

  // Verify env lockfile was created with expected content in pnpm-lock.yaml
  const envLockfile = (await readEnvLockfile(process.cwd()))!
  expect(envLockfile.lockfileVersion).toBeDefined()
  expect(envLockfile.importers['.'].configDependencies['@pnpm.e2e/foo']).toEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect((envLockfile.packages['@pnpm.e2e/foo@100.0.0'].resolution as { integrity: string }).integrity).toBe(integrity)
})

test('migration fails with frozenLockfile when no env lockfile exists', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const integrity = getIntegrity('@pnpm.e2e/foo', '100.0.0')
  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': `100.0.0+${integrity}`,
  }
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
    frozenLockfile: true,
  })).rejects.toThrow('Cannot migrate configDependencies with "frozen-lockfile"')
})

test('installation fails if the config dependency does not have a checksum (old format)', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore({
    clientOptions: {
      retry: {
        retries: 0,
      },
    },
  })

  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': '100.0.0',
  }
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })).rejects.toThrow('already in clean-specifier form')
})
