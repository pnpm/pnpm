import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { installConfigDeps } from '@pnpm/installing.env-installer'
import { createEnvLockfile, type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { createTempStore } from '@pnpm/testing.temp-store'
import { rimraf } from '@zkochan/rimraf'
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

test('a config dependency with a path-traversal name in the env lockfile is rejected', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const maliciousName = '../../PWNED'
  const lockfile = makeEnvLockfile({
    [maliciousName]: { version: '1.0.0', integrity: getIntegrity('@pnpm.e2e/foo', '100.0.0') },
  })

  await expect(installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })).rejects.toThrow('invalid name')

  // No path is created outside node_modules/.pnpm-config.
  expect(fs.existsSync(path.resolve(process.cwd(), '../../PWNED'))).toBe(false)
  expect(fs.existsSync(path.resolve(process.cwd(), 'PWNED'))).toBe(false)
})

test('an optional subdep with a path-traversal name in the env lockfile is rejected', async () => {
  prepareEmpty()
  const { storeController, storeDir } = createTempStore()

  const parentName = '@pnpm.e2e/foo'
  const parentVersion = '100.0.0'
  const maliciousSubdepName = '../../PWNED_SUBDEP'
  const subdepVersion = '1.0.0'

  const lockfile = createEnvLockfile()
  const parentKey = `${parentName}@${parentVersion}`
  lockfile.importers['.'].configDependencies[parentName] = { specifier: parentVersion, version: parentVersion }
  lockfile.packages[parentKey] = { resolution: { integrity: getIntegrity(parentName, parentVersion) } }
  lockfile.snapshots[parentKey] = {
    optionalDependencies: { [maliciousSubdepName]: subdepVersion },
  }
  lockfile.packages[`${maliciousSubdepName}@${subdepVersion}`] = {
    resolution: { integrity: getIntegrity('@pnpm.e2e/bar', '100.0.0') },
  }

  await expect(installConfigDeps(lockfile, {
    registries: {
      default: registry,
    },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  })).rejects.toThrow('invalid name')

  expect(fs.existsSync(path.resolve(process.cwd(), '../../PWNED_SUBDEP'))).toBe(false)
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

test('re-installs sibling symlinks even when the parent symlink is already correct', async () => {
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
  lockfile.snapshots[parentKey] = { optionalDependencies: { [subdepName]: subdepVersion } }
  lockfile.packages[`${subdepName}@${subdepVersion}`] = {
    resolution: { integrity: getIntegrity(subdepName, subdepVersion) },
    os: [process.platform],
    cpu: [process.arch],
  }

  const installOpts = {
    registries: { default: registry },
    rootDir: process.cwd(),
    store: storeController,
    storeDir,
  }

  // First install — parent + subdep symlink land in the GVS leaf.
  await installConfigDeps(lockfile, installOpts)
  const parentRealPath = fs.realpathSync(`node_modules/.pnpm-config/${parentName}`)
  const subdepSiblingPath = path.join(path.dirname(path.dirname(parentRealPath)), subdepName)
  expect(fs.existsSync(`${subdepSiblingPath}/package.json`)).toBe(true)

  // Simulate stale state: remove the subdep sibling symlink. The parent's
  // .pnpm-config symlink still points at the expected leaf, so the realpath
  // skip-check passes. installOptionalSubdeps must still run to repair.
  await rimraf(subdepSiblingPath)
  expect(fs.existsSync(subdepSiblingPath)).toBe(false)

  // Second install with the same lockfile.
  await installConfigDeps(lockfile, installOpts)
  expect(fs.existsSync(`${subdepSiblingPath}/package.json`)).toBe(true)
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
