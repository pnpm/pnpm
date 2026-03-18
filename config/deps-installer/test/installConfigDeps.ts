import fs from 'node:fs'

import { installConfigDeps } from '@pnpm/config.deps-installer'
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
