import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test('merging branch lockfiles preserves a workspace directory named __proto__', async () => {
  preparePackages([
    { location: '.', package: { name: 'root' } },
    { location: '__proto__', package: { name: 'app', dependencies: { foo: 'link:../foo' } } },
    { location: 'foo', package: { name: 'foo', version: '1.0.0' } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['*'] })
  writeYamlFileSync('pnpm-lock.yaml', {
    lockfileVersion: '9.0',
    settings: { autoInstallPeers: true, excludeLinksFromLockfile: false },
    importers: { '.': {}, foo: {} },
  })
  writeYamlFileSync('pnpm-lock.branch.yaml', JSON.parse('{"lockfileVersion":"9.0","importers":{"__proto__":{"dependencies":{"foo":{"specifier":"link:../foo","version":"link:../foo"}}}}}'))

  await execPnpm(['install', '--merge-git-branch-lockfiles', '--frozen-lockfile', '--lockfile-only'])

  const lockfile = readYamlFileSync<{ importers: Record<string, unknown> }>('pnpm-lock.yaml')!
  expect(Object.hasOwn(lockfile.importers, '__proto__')).toBe(true)
  expect(lockfile.importers.__proto__).toStrictEqual({
    dependencies: { foo: { specifier: 'link:../foo', version: 'link:../foo' } },
  })
})

test('merging branch lockfiles preserves publishDirectory, linkDirectory, and dependenciesMeta under --frozen-lockfile', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'my-app',
        version: '1.0.0',
        publishConfig: {
          directory: 'dist',
          linkDirectory: false,
        },
        dependencies: {
          'local-dep': 'file:./local-dep',
        },
        dependenciesMeta: {
          'local-dep': {
            injected: true,
          },
        },
      },
    },
    {
      location: 'local-dep',
      package: {
        name: 'local-dep',
        version: '1.0.0',
      },
    },
  ])
  fs.mkdirSync('dist', { recursive: true })

  await execPnpm(['install'])
  fs.copyFileSync('pnpm-lock.yaml', 'pnpm-lock.feature.yaml')

  await execPnpm(['install', '--merge-git-branch-lockfiles', '--frozen-lockfile'])

  expect(fs.existsSync('pnpm-lock.feature.yaml')).toBe(false)
  const lockfile = readYamlFileSync<{ importers: Record<string, Record<string, unknown>> }>('pnpm-lock.yaml')!
  expect(lockfile.importers['.'].publishDirectory).toBe('dist')
  expect(lockfile.importers['.'].linkDirectory).toBe(false)
  expect(lockfile.importers['.'].dependenciesMeta).toStrictEqual({
    'local-dep': { injected: true },
  })
})
