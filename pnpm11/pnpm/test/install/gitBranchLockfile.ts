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
