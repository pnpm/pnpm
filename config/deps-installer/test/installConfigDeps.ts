import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { installConfigDeps } from '@pnpm/config.deps-installer'
import { sync as loadJsonFile } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

test('configuration dependency is installed', async () => {
  prepareEmpty()
  const { storeController } = createTempStore()

  let configDeps: Record<string, string> = {
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
    const configDepManifest = loadJsonFile<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.0.0')
  }

  // Dependency is updated
  configDeps!['@pnpm.e2e/foo'] = `100.1.0+${getIntegrity('@pnpm.e2e/foo', '100.1.0')}`

  await installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })

  {
    const configDepManifest = loadJsonFile<{ name: string, version: string }>('node_modules/.pnpm-config/@pnpm.e2e/foo/package.json')
    expect(configDepManifest.name).toBe('@pnpm.e2e/foo')
    expect(configDepManifest.version).toBe('100.1.0')
  }

  // Dependency is removed
  configDeps! = {}

  await installConfigDeps(configDeps, {
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

  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': '100.0.0+sha512-00000000000000000000000000000000000000000000000000000000000000000000000000000000000000==',
  }
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })).rejects.toThrow('Got unexpected checksum for')
})

test('installation fails if the config dependency does not have a checksum', async () => {
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
