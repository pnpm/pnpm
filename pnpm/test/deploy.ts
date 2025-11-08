import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
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

  writeYamlFile('pnpm-workspace.yaml', {
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
