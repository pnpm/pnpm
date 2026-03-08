import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { installConfigDeps, createConfigLockfile, type ConfigLockfile } from '@pnpm/config.deps-installer'
import { loadJsonFileSync } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

function makeConfigLockfile (deps: Record<string, { version: string, integrity: string }>): ConfigLockfile {
  const lockfile = createConfigLockfile()
  for (const [name, { version, integrity }] of Object.entries(deps)) {
    const pkgKey = `${name}@${version}`
    lockfile.importers['.'].configDependencies[name] = { specifier: version, version }
    lockfile.packages[pkgKey] = { resolution: { integrity } }
    lockfile.snapshots[pkgKey] = {}
  }
  return lockfile
}

test('configuration dependency is installed from config lockfile', async () => {
  prepareEmpty()
  const { storeController } = createTempStore()

  const lockfile = makeConfigLockfile({
    '@pnpm.e2e/foo': { version: '100.0.0', integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  })
  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
  }

  // Dependency is updated
  const lockfile2 = makeConfigLockfile({
    '@pnpm.e2e/foo': { version: '100.1.0', integrity: getIntegrity('@pnpm.e2e/foo', '100.1.0') },
  })

  await installConfigDeps(lockfile2, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.1.0')
  }

  // Dependency is removed
  const lockfile3 = createConfigLockfile()

  await installConfigDeps(lockfile3, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })

  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')).toBeFalsy()
})

test('installation fails if the checksum of the config dependency is invalid', async () => {
  prepareEmpty()
  const { storeController } = createTempStore({
    clientOptions: {
      retry: {
        retries: 0,
      },
    },
  })

  const lockfile = makeConfigLockfile({
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
  })).rejects.toThrow('Got unexpected checksum for')
})

test('migration: installs from old inline integrity format and creates config lockfile', async () => {
  prepareEmpty()
  const { storeController } = createTempStore()

  // Old format: ConfigDependencies with inline integrity
  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  }
  await installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })

  {
    const configDepManifest = loadJsonFileSync<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
  }
})

test('installation fails if the config dependency does not have a checksum (old format)', async () => {
  prepareEmpty()
  const { storeController } = createTempStore({
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
  })).rejects.toThrow("doesn't have an integrity checksum")
})
