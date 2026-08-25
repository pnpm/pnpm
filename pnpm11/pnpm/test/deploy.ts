import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

// Covers https://github.com/pnpm/pnpm/issues/9550
// This test is currently disabled because of https://github.com/pnpm/pnpm/issues/9596
test.skip('legacy deploy creates only necessary directories when the root manifest has a workspace package as a peer dependency (#9550)', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        peerDependencies: {
          bar: 'workspace:^',
        },
      },
    },
    {
      location: 'services/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        dependencies: {
          '@pnpm.e2e/foo': '^100.1.0',
          bar: 'workspace:*',
        },
      },
    },
    {
      location: 'packages/bar',
      package: {
        name: 'bar',
        version: '0.0.0',
        dependencies: {
          '@pnpm.e2e/bar': '^100.1.0',
        },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: [
      'services/*',
      'packages/*',
    ],
    forceLegacyDeploy: true,
    shamefullyHoist: true,
    linkWorkspacePackages: true,
    reporter: 'append-only',
    storeDir: path.resolve('pnpm-store'),
    cacheDir: path.resolve('pnpm-cache'),
  })

  await execPnpm(['install'])
  expect(fs.realpathSync('node_modules/bar')).toBe(path.resolve('packages/bar'))
  const beforeDeploy = {
    '.': fs.readdirSync('.').sort(),
    services: fs.readdirSync('services').sort(),
    'services/foo': fs.readdirSync('services/foo').sort(),
    packages: fs.readdirSync('packages').sort(),
    'packages/bar': fs.readdirSync('packages/bar').sort(),
  }

  await execPnpm(['--filter=foo', 'deploy', 'services/foo/pnpm.out'])
  const afterDeploy = {
    '.': fs.readdirSync('.').sort(),
    services: fs.readdirSync('services').sort(),
    'services/foo': fs.readdirSync('services/foo').sort(),
    packages: fs.readdirSync('packages').sort(),
    'packages/bar': fs.readdirSync('packages/bar').sort(),
  }

  expect(afterDeploy).toStrictEqual({
    ...beforeDeploy,
    'services/foo': [
      ...beforeDeploy['services/foo'],
      'pnpm.out',
    ].sort(),
  })
  expect(fs.readdirSync('services/foo/pnpm.out').sort()).toStrictEqual(['node_modules', 'package.json'])
  expect(loadJsonFileSync('services/foo/pnpm.out/package.json')).toStrictEqual(loadJsonFileSync('services/foo/package.json'))
})

test('deploy with a shared lockfile honors --no-optional in the graph and virtual store', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '0.0.0', private: true } },
    {
      location: 'packages/app',
      package: {
        name: 'app',
        version: '1.0.0',
        dependencies: {
          lib: 'workspace:*',
          '@pnpm.e2e/support-different-architectures': '1.0.0',
        },
        optionalDependencies: { 'optional-only': 'workspace:*' },
      },
    },
    {
      location: 'packages/lib',
      package: {
        name: 'lib',
        version: '1.0.0',
        optionalDependencies: { '@pnpm.e2e/qar': '100.0.0' },
      },
    },
    {
      location: 'packages/optional-only',
      package: {
        name: 'optional-only',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/foo': '100.0.0' },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    injectWorkspacePackages: true,
  })

  await execPnpm(['install'])

  const deployDir = path.resolve('deploy-without-optional')
  await execPnpm(['--filter=app', 'deploy', '--prod', '--no-optional', deployDir])

  expect(fs.existsSync(path.join(deployDir, 'node_modules/lib'))).toBe(true)
  expect(fs.existsSync(path.join(deployDir, 'node_modules/optional-only'))).toBe(false)

  const virtualStoreEntries = fs.readdirSync(path.join(deployDir, 'node_modules/.pnpm'))
  for (const excluded of ['optional-only@file+', '@pnpm.e2e+qar@', '@pnpm.e2e+foo@']) {
    expect(virtualStoreEntries.filter(entry => entry.includes(excluded))).toStrictEqual([])
  }

  const deployLockfile = readYamlFileSync<LockfileFile>(path.join(deployDir, 'pnpm-lock.yaml'))
  const retainedOptionalEdges = Object.entries(deployLockfile.snapshots ?? {})
    .filter(([, snapshot]) => snapshot.optionalDependencies != null)
  expect(retainedOptionalEdges).toStrictEqual([])
})

// A deployed lockfile must never reference a package the graph filter drops:
// a later install in the deploy directory would link the missing package and
// leave the dangling symlinks of https://github.com/pnpm/pnpm/issues/13623.
test('deploy with a shared lockfile drops excluded direct dependency groups', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '0.0.0', private: true } },
    {
      location: 'packages/app',
      package: {
        name: 'app',
        version: '1.0.0',
        dependencies: { '@pnpm.e2e/foo': '100.0.0' },
        devDependencies: { '@pnpm.e2e/bar': '100.0.0' },
        optionalDependencies: { '@pnpm.e2e/qar': '100.0.0' },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    injectWorkspacePackages: true,
  })

  await execPnpm(['install'])

  // Deploying outside the workspace keeps the follow-up install standalone.
  const workspaceDir = process.cwd()
  const deployDir = path.join(tempDir(false), 'deploy')
  await execPnpm(['--filter=app', 'deploy', '--prod', '--no-optional', deployDir])

  const deployManifest = loadJsonFileSync<Record<string, unknown>>(path.join(deployDir, 'package.json'))
  expect(deployManifest.dependencies).toStrictEqual({ '@pnpm.e2e/foo': '100.0.0' })
  expect(deployManifest.devDependencies).toStrictEqual({})
  expect(deployManifest.optionalDependencies).toStrictEqual({})

  const deployLockfile = readYamlFileSync<LockfileFile>(path.join(deployDir, 'pnpm-lock.yaml'))
  const importer = deployLockfile.importers!['.']
  expect(importer.devDependencies ?? {}).toStrictEqual({})
  expect(importer.optionalDependencies ?? {}).toStrictEqual({})
  const graphKeys = Object.keys(deployLockfile.packages ?? {})
  expect(graphKeys.filter(key => key.startsWith('@pnpm.e2e/bar@') || key.startsWith('@pnpm.e2e/qar@'))).toStrictEqual([])

  const nodeModulesDir = path.join(deployDir, 'node_modules')
  fs.rmSync(nodeModulesDir, { recursive: true })
  process.chdir(deployDir)
  try {
    await execPnpm(['install', '--frozen-lockfile'])
  } finally {
    process.chdir(workspaceDir)
  }
  const dangling = fs.readdirSync(nodeModulesDir, { recursive: true, encoding: 'utf8' })
    .filter(entry => !fs.existsSync(path.join(nodeModulesDir, entry)))
  expect(dangling).toStrictEqual([])
})

// `pacquet` is fetched from the real npm registry — registry-mock doesn't
// carry it (or its platform-specific binary sub-packages), so this test
// requires the public registry to be reachable. Matches the pattern in
// `pnpm/test/install/pacquet.ts`.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'
const PACQUET_VERSION = '0.2.2'

// Two installs against the public registry plus a deploy; raise the per-test
// timeout above jest's 5s default to allow for cold caches.
const PUBLIC_REGISTRY_TIMEOUT = 5 * 60 * 1000

test('deploy with a shared lockfile succeeds when pacquet is declared in configDependencies', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '0.0.0' } },
    {
      location: 'services/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        dependencies: { 'is-positive': '3.1.0' },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['services/*'],
    configDependencies: { pacquet: PACQUET_VERSION },
    injectWorkspacePackages: true,
  })

  await execPnpm([PUBLIC_REGISTRY, 'install'])

  const deployDir = path.resolve('services/foo/pnpm.out')
  await execPnpm([PUBLIC_REGISTRY, '--filter=foo', 'deploy', deployDir])

  expect(fs.existsSync(path.join(deployDir, 'node_modules/is-positive/package.json'))).toBe(true)
}, PUBLIC_REGISTRY_TIMEOUT)
