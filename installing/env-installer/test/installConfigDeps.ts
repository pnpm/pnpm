import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

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

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const subdepName = '@pnpm.e2e/bar'
  const subdepVersion = '100.0.0'

  const lockfile = createEnvLockfile()
  const parentKey = `${parentName}@${parentVersion}`
  lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
  lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
  lockfile.snapshots[parentKey] = {
    optionalDependencies: { [subdepName]: subdepVersion },
  }
  lockfile.packages[`${subdepName}@${subdepVersion}`] = {
    resolution: { integrity: getIntegrity(subdepName, subdepVersion) },
    os: [process.platform],
    cpu: [process.arch],
  }

  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  expect(fs.existsSync(`node_modules/.pnpm-config/${parentName}/package.json`)).toBe(true)

  // Node-style resolution from inside the parent must find the sibling subdep.
  const parentRealPath = fs.realpathSync(`node_modules/.pnpm-config/${parentName}`)
  const requireFromParent = createRequire(path.join(parentRealPath, 'package.json'))
  const siblingPkgJsonPath = requireFromParent.resolve(`${subdepName}/package.json`)
  const siblingManifest = loadJsonFileSync<{ name: string, version: string }>(siblingPkgJsonPath)
  expect(siblingManifest.name).toBe(subdepName)
  expect(siblingManifest.version).toBe(subdepVersion)
})

test('changing only an optional subdep version re-installs and re-symlinks the parent', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const subdepName = '@pnpm.e2e/bar'

  function buildLockfile (subdepVersion: string): EnvLockfile {
    const lockfile = createEnvLockfile()
    const parentKey = `${parentName}@${parentVersion}`
    lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
    lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
    lockfile.snapshots[parentKey] = { optionalDependencies: { [subdepName]: subdepVersion } }
    lockfile.packages[`${subdepName}@${subdepVersion}`] = {
      resolution: { integrity: getIntegrity(subdepName, subdepVersion) },
      os: [process.platform],
      cpu: [process.arch],
    }
    return lockfile
  }

  const installOpts = {
    registries: { default: registry },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  }

  await installConfigDeps(buildLockfile('100.0.0'), installOpts)
  const requireBefore = createRequire(path.join(fs.realpathSync(`node_modules/.pnpm-config/${parentName}`), 'package.json'))
  expect(loadJsonFileSync<{ version: string }>(requireBefore.resolve(`${subdepName}/package.json`)).version).toBe('100.0.0')

  await installConfigDeps(buildLockfile('100.1.0'), installOpts)
  const requireAfter = createRequire(path.join(fs.realpathSync(`node_modules/.pnpm-config/${parentName}`), 'package.json'))
  expect(loadJsonFileSync<{ version: string }>(requireAfter.resolve(`${subdepName}/package.json`)).version).toBe('100.1.0')
})

test('optional subdep that does not match the current platform is skipped', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const subdepName = '@pnpm.e2e/bar'
  const subdepVersion = '100.0.0'

  const lockfile = createEnvLockfile()
  const parentKey = `${parentName}@${parentVersion}`
  lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
  lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
  lockfile.snapshots[parentKey] = {
    optionalDependencies: { [subdepName]: subdepVersion },
  }
  lockfile.packages[`${subdepName}@${subdepVersion}`] = {
    resolution: { integrity: getIntegrity(subdepName, subdepVersion) },
    os: ['this-os-does-not-exist'],
  }

  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  const parentRealPath = fs.realpathSync(`node_modules/.pnpm-config/${parentName}`)
  const requireFromParent = createRequire(path.join(parentRealPath, 'package.json'))
  expect(() => requireFromParent.resolve(`${subdepName}/package.json`)).toThrow(/Cannot find/)
})

test('platform change between runs prunes the stale sibling and relinks the new compatible one', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const subdepA = { name: '@pnpm.e2e/bar', version: '100.0.0' }
  const subdepB = { name: '@pnpm.e2e/qar', version: '100.0.0' }

  // Same parent + same subdep versions → same leaf hash both runs. Only the
  // os field on each subdep changes, which the lockfile's leaf hash doesn't
  // capture but the install-time selector does. The realpath skip-check
  // would match, so we rely on installOptionalSubdeps running unconditionally.
  function buildLockfile (matching: { name: string, version: string }, other: { name: string, version: string }): EnvLockfile {
    const lockfile = createEnvLockfile()
    const parentKey = `${parentName}@${parentVersion}`
    lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
    lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
    lockfile.snapshots[parentKey] = {
      optionalDependencies: {
        [matching.name]: matching.version,
        [other.name]: other.version,
      },
    }
    lockfile.packages[`${matching.name}@${matching.version}`] = {
      resolution: { integrity: getIntegrity(matching.name, matching.version) },
      os: [process.platform],
    }
    lockfile.packages[`${other.name}@${other.version}`] = {
      resolution: { integrity: getIntegrity(other.name, other.version) },
      os: ['this-os-does-not-exist'],
    }
    return lockfile
  }

  const installOpts = {
    registries: { default: registry },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  }

  // First run: A compatible, B incompatible.
  await installConfigDeps(buildLockfile(subdepA, subdepB), installOpts)
  const requireRun1 = createRequire(path.join(fs.realpathSync(`node_modules/.pnpm-config/${parentName}`), 'package.json'))
  expect(() => requireRun1.resolve(`${subdepA.name}/package.json`)).not.toThrow()
  expect(() => requireRun1.resolve(`${subdepB.name}/package.json`)).toThrow(/Cannot find/)

  // Second run, roles swapped: B compatible, A incompatible. Same leaf hash.
  await installConfigDeps(buildLockfile(subdepB, subdepA), installOpts)
  const requireRun2 = createRequire(path.join(fs.realpathSync(`node_modules/.pnpm-config/${parentName}`), 'package.json'))
  expect(() => requireRun2.resolve(`${subdepB.name}/package.json`)).not.toThrow()
  expect(() => requireRun2.resolve(`${subdepA.name}/package.json`)).toThrow(/Cannot find/)
})

test('optional subdep that does not match the current cpu is skipped', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const subdepName = '@pnpm.e2e/bar'
  const subdepVersion = '100.0.0'

  const lockfile = createEnvLockfile()
  const parentKey = `${parentName}@${parentVersion}`
  lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
  lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
  lockfile.snapshots[parentKey] = {
    optionalDependencies: { [subdepName]: subdepVersion },
  }
  lockfile.packages[`${subdepName}@${subdepVersion}`] = {
    resolution: { integrity: getIntegrity(subdepName, subdepVersion) },
    cpu: ['this-cpu-does-not-exist'],
  }

  await installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })

  const parentRealPath = fs.realpathSync(`node_modules/.pnpm-config/${parentName}`)
  const requireFromParent = createRequire(path.join(parentRealPath, 'package.json'))
  expect(() => requireFromParent.resolve(`${subdepName}/package.json`)).toThrow(/Cannot find/)
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
