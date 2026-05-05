import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeJsonFileSync } from 'write-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync, pnpmBinLocation } from './utils/index.js'

test('patch from configuration dependency is applied', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/has-patch-for-foo': `1.0.0+${getIntegrity('@pnpm.e2e/has-patch-for-foo', '1.0.0')}`,
    },
    patchedDependencies: {
      '@pnpm.e2e/foo@100.0.0': 'node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo/@pnpm.e2e__foo@100.0.0.patch',
    },
  })

  await execPnpm(['add', '@pnpm.e2e/foo@100.0.0'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/foo/index.js')).toBeTruthy()
})

test('patch from configuration dependency is applied via updateConfig hook', async () => {
  const project = prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/has-patch-for-foo': `1.0.0+${getIntegrity('@pnpm.e2e/has-patch-for-foo', '1.0.0')}`,
    },
    pnpmfile: 'node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo/pnpmfile.cjs',
  })

  await execPnpm(['add', '@pnpm.e2e/foo@100.0.0'])

  expect(fs.existsSync('node_modules/@pnpm.e2e/foo/index.js')).toBeTruthy()

  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies['@pnpm.e2e/foo']).toEqual(expect.any(String))
})

test('catalog applied by configurational dependency hook', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
      '@pnpm.e2e/bar': 'catalog:bar',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/update-config-with-catalogs': `1.0.0+${getIntegrity('@pnpm.e2e/update-config-with-catalogs', '1.0.0')}`,
    },
    pnpmfile: 'node_modules/.pnpm-config/@pnpm.e2e/update-config-with-catalogs/pnpmfile.cjs',
  })

  await execPnpm(['install'])

  const lockfile = project.readLockfile()
  expect(lockfile.catalogs).toStrictEqual({
    bar: {
      '@pnpm.e2e/bar': {
        specifier: '100.0.0',
        version: '100.0.0',
      },
    },
    default: {
      '@pnpm.e2e/foo': {
        specifier: '100.0.0',
        version: '100.0.0',
      },
    },
  })
})

test('config deps are not installed before switching to a different pnpm version', async () => {
  prepare()
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }

  // First, add config dep to create the env lockfile (clean specifier format)
  await execPnpm(['add', '@pnpm.e2e/has-patch-for-foo@1.0.0', '--config'], { env })

  // Remove node_modules so we can check if config deps get re-installed
  fs.rmSync('node_modules', { recursive: true })

  // Switch to pnpm 9.3.0, which doesn't know about configDependencies.
  // If the current pnpm installed config deps before switching, the directory would exist.
  writeJsonFileSync('package.json', {
    packageManager: 'pnpm@9.3.0',
  })

  execPnpmSync(['install'], { env, stdio: 'pipe' })

  // Config deps should NOT be installed — pnpm 9.3.0 doesn't support them,
  // and the current pnpm should not have installed them before switching.
  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo')).toBeFalsy()
})

test('config deps are installed after switching to a pnpm version that supports them', async () => {
  prepare({
    packageManager: 'pnpm@10.32.0',
  })
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }
  // Write .npmrc so the switched-to pnpm version can find the mock registry
  fs.writeFileSync('.npmrc', `registry=http://localhost:${REGISTRY_MOCK_PORT}/\n`)
  // Use old inline integrity format that pnpm v10 understands
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/has-patch-for-foo': `1.0.0+${getIntegrity('@pnpm.e2e/has-patch-for-foo', '1.0.0')}`,
    },
  })

  execPnpmSync(['install'], { env })

  // pnpm 10.32.0 supports configDependencies and should have installed them
  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo')).toBeTruthy()
})

test('package manager from the packageManager field is not saved into the lockfile', async () => {
  const pnpmVersion = JSON.parse(fs.readFileSync(path.join(path.dirname(pnpmBinLocation), '..', 'package.json'), 'utf8')).version as string
  prepare({
    packageManager: `pnpm@${pnpmVersion}`,
  })
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }

  // Create the env lockfile via pnpm add --config
  await execPnpm(['add', '@pnpm.e2e/has-patch-for-foo@1.0.0', '--config'], { env })

  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo')).toBeTruthy()

  // The legacy packageManager field already pins an exact version in the
  // manifest itself, so pnpm resolution info must not leak into the lockfile.
  // Config dependencies are still persisted because they are managed by an
  // independent code path.
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/has-patch-for-foo']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })
  expect(envLockfile!.importers['.'].packageManagerDependencies).toBeUndefined()
})

test('packageManagerDependencies is refreshed when pnpm is invoked via corepack (#11397)', async () => {
  const pnpmVersion = JSON.parse(fs.readFileSync(path.join(path.dirname(pnpmBinLocation), '..', 'package.json'), 'utf8')).version as string
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: pnpmVersion,
      },
    },
  })

  // Seed the lockfile with a stale packageManagerDependencies entry that no
  // longer satisfies devEngines.packageManager. Multi-document YAML: env
  // lockfile is the first doc, the (empty) installer lockfile is the second.
  fs.writeFileSync('pnpm-lock.yaml', `---
lockfileVersion: '9.0'
importers:
  '.':
    configDependencies: {}
    packageManagerDependencies:
      pnpm:
        specifier: 0.0.1
        version: 0.0.1
packages: {}
snapshots: {}

---
`)

  // COREPACK_ROOT used to skip the entire pm-handling block, leaving the stale
  // 0.0.1 entry untouched. The sync must run regardless of how pnpm was
  // invoked.
  await execPnpm(['install'], {
    env: { COREPACK_ROOT: '/fake/corepack' },
  })

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].packageManagerDependencies?.['pnpm'].version).toBe(pnpmVersion)
})

test('installing a new configurational dependency', async () => {
  prepare()

  await execPnpm(['add', '@pnpm.e2e/foo@100.0.0', '--config'])

  // Workspace manifest should have a clean specifier (no integrity)
  const workspaceManifest = readYamlFileSync<{ configDependencies: Record<string, string> }>('pnpm-workspace.yaml')
  expect(workspaceManifest.configDependencies).toStrictEqual({
    '@pnpm.e2e/foo': '100.0.0',
  })

  // Env lockfile should contain the resolved dependency with integrity
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/foo']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
  expect((envLockfile!.packages['@pnpm.e2e/foo@100.0.0'].resolution as { integrity: string }).integrity).toBe(
    getIntegrity('@pnpm.e2e/foo', '100.0.0')
  )
})

// Regression tests for https://github.com/pnpm/pnpm/issues/10684 — if the user
// has a configDependency stored in a registry that needs auth, the config
// commands must not crash when pnpm tries to fetch the configDependency before
// the new setting is written. We reference a non-existent package version so
// the install errors out fast; the real-world scenario is a 401 from the
// private registry.
//
// All four entry points are tested: `pnpm config set`, `pnpm config get`,
// `pnpm set`, and `pnpm get`. The latter two are shortcuts that delegate to
// the config handler internally but are separate top-level commands, so they
// need their own coverage at the main.ts guard level.
function writeFailingConfigDep () {
  // Clean specifier for a version that does not exist on the mock registry.
  // fetchRetries: 0 keeps the failure fast so the test does not time out.
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/foo': '999.999.999',
    },
    fetchRetries: 0,
  })
}

test('pnpm config set succeeds even when configDependencies fail to install', async () => {
  prepare()
  writeFailingConfigDep()

  // Use an auth-style key so the setting lands in ./.npmrc (project scope).
  const authKey = '//example.com/:_authToken'
  await execPnpm(['config', 'set', '--location=project', authKey, 'my-secret-token'])

  const npmrc = fs.readFileSync('.npmrc', 'utf8')
  expect(npmrc).toContain(`${authKey}=my-secret-token`)
})

test('pnpm config get succeeds even when configDependencies fail to install', async () => {
  prepare()
  writeFailingConfigDep()
  const authKey = '//example.com/:_authToken'
  fs.writeFileSync('.npmrc', `${authKey}=my-secret-token\n`, 'utf8')

  const result = execPnpmSync(['config', 'get', '--location=project', authKey], { expectSuccess: true })
  expect(result.stdout.toString()).toContain('my-secret-token')
})

test('pnpm set succeeds even when configDependencies fail to install', async () => {
  prepare()
  writeFailingConfigDep()

  const authKey = '//example.com/:_authToken'
  await execPnpm(['set', '--location=project', authKey, 'my-secret-token'])

  const npmrc = fs.readFileSync('.npmrc', 'utf8')
  expect(npmrc).toContain(`${authKey}=my-secret-token`)
})

test('pnpm get succeeds even when configDependencies fail to install', async () => {
  prepare()
  writeFailingConfigDep()
  const authKey = '//example.com/:_authToken'
  fs.writeFileSync('.npmrc', `${authKey}=my-secret-token\n`, 'utf8')

  const result = execPnpmSync(['get', '--location=project', authKey], { expectSuccess: true })
  expect(result.stdout.toString()).toContain('my-secret-token')
})
