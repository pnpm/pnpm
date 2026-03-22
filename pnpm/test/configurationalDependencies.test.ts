import fs from 'node:fs'
import path from 'node:path'

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

test('package manager is saved into the lockfile even if it matches the current version', async () => {
  const pnpmVersion = JSON.parse(fs.readFileSync(path.join(path.dirname(pnpmBinLocation), '..', 'package.json'), 'utf8')).version as string
  prepare({
    packageManager: `pnpm@${pnpmVersion}`,
  })
  const pnpmHome = path.resolve('pnpm')
  const env = { PNPM_HOME: pnpmHome }

  // Create the env lockfile via pnpm add --config
  await execPnpm(['add', '@pnpm.e2e/has-patch-for-foo@1.0.0', '--config'], { env })

  expect(fs.existsSync('node_modules/.pnpm-config/@pnpm.e2e/has-patch-for-foo')).toBeTruthy()

  // The env lockfile should have both config dep and package manager entries
  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile).not.toBeNull()
  expect(envLockfile!.importers['.'].configDependencies['@pnpm.e2e/has-patch-for-foo']).toStrictEqual({
    specifier: '1.0.0',
    version: '1.0.0',
  })
  expect(envLockfile!.importers['.'].packageManagerDependencies).toBeDefined()
  expect(envLockfile!.importers['.'].packageManagerDependencies!['pnpm']).toStrictEqual({
    specifier: pnpmVersion,
    version: pnpmVersion,
  })
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
