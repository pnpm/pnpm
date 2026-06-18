import fs from 'fs'
import path from 'path'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { installConfigDeps } from '@pnpm/config.deps-installer'
import { sync as loadJsonFile } from 'load-json-file'

const registry = `http://localhost:${REGISTRY_MOCK_PORT}/`

// Recursively search `dir` for an entry named `name`, without following
// symlinks (so it can't loop through the links a successful install creates).
function containsEntryNamed (dir: string, name: string): boolean {
  let entries: fs.Dirent[]
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true })
  } catch {
    return false
  }
  for (const entry of entries) {
    if (entry.name === name) return true
    if (entry.isDirectory() && containsEntryNamed(path.join(dir, entry.name), name)) return true
  }
  return false
}

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

test('a config dependency with a path-traversal name is rejected', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const configDeps: Record<string, string> = {
    '../../PWNED': `100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  }
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })).rejects.toThrow('invalid name')

  expect(containsEntryNamed(process.cwd(), 'PWNED')).toBe(false)
  expect(containsEntryNamed(storeDir, 'PWNED')).toBe(false)
})

test('a config dependency named __proto__ is rejected', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  // JSON.parse makes `__proto__` an own enumerable key (as on-disk parsing
  // does); a plain object literal would set the prototype and hide it.
  const configDeps: Record<string, string> = JSON.parse(
    `{"__proto__":"100.0.0+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}"}`
  )
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })).rejects.toThrow('invalid name')

  expect(containsEntryNamed(process.cwd(), '__proto__')).toBe(false)
  expect(containsEntryNamed(storeDir, '__proto__')).toBe(false)
})

test('a config dependency with a path-traversal version is rejected', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const configDeps: Record<string, string> = {
    '@pnpm.e2e/foo': `../../../PWNED+${getIntegrity('@pnpm.e2e/foo', '100.0.0')}`,
  }
  await expect(installConfigDeps(configDeps, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
  })).rejects.toThrow('invalid version')

  expect(containsEntryNamed(process.cwd(), 'PWNED')).toBe(false)
  expect(containsEntryNamed(storeDir, 'PWNED')).toBe(false)
})
