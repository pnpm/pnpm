import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import type { LockfileFile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { rimrafSync } from '@zkochan/rimraf'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from '../utils/index.js'

// `@pnpm.e2e/pkg-with-internal-file-dep` declares
// `"@pnpm.e2e/internal-child": "file:./child"` and ships `child/` in its
// tarball, the way `@eslint/css@0.3.0` shipped `typings/css-tree`.
const manifest = {
  dependencies: {
    '@pnpm.e2e/pkg-with-internal-file-dep': '1.0.0',
  },
}

test('a file: dependency pointing inside a registry package is linked to that directory', async () => {
  const project = prepareEmpty()

  await install(manifest, testDefaults())

  expect(project.requireModule('@pnpm.e2e/pkg-with-internal-file-dep')()).toBe('internal child')
  const lockfile = readYamlFileSync<LockfileFile>(path.resolve('pnpm-lock.yaml'))
  expect(lockfile.snapshots!['@pnpm.e2e/pkg-with-internal-file-dep@1.0.0'].dependencies).toStrictEqual({
    '@pnpm.e2e/internal-child': 'link:<root>/child',
  })

  rimrafSync('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }))

  expect(project.requireModule('@pnpm.e2e/pkg-with-internal-file-dep')()).toBe('internal child')

  await install(manifest, testDefaults({ frozenLockfile: false, preferFrozenLockfile: false }))

  expect(readYamlFileSync<LockfileFile>(path.resolve('pnpm-lock.yaml'))).toStrictEqual(lockfile)
})

test('a file: dependency pointing inside a registry package is linked with the hoisted node-linker', async () => {
  const project = prepareEmpty()

  await install(manifest, testDefaults({ nodeLinker: 'hoisted' }))

  expect(project.requireModule('@pnpm.e2e/pkg-with-internal-file-dep')()).toBe('internal child')
  expect(fs.realpathSync('node_modules/@pnpm.e2e/pkg-with-internal-file-dep/node_modules/@pnpm.e2e/internal-child'))
    .toBe(fs.realpathSync('node_modules/@pnpm.e2e/pkg-with-internal-file-dep/child'))
})

test('a file: dependency pointing inside a registry package is linked in the global virtual store', async () => {
  const project = prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const opts = { enableGlobalVirtualStore: true, virtualStoreDir: globalVirtualStoreDir }

  await install(manifest, testDefaults(opts))

  expect(project.requireModule('@pnpm.e2e/pkg-with-internal-file-dep')()).toBe('internal child')

  rimrafSync('node_modules')
  await install(manifest, testDefaults({ ...opts, frozenLockfile: true }))

  expect(project.requireModule('@pnpm.e2e/pkg-with-internal-file-dep')()).toBe('internal child')
  expect(fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-internal-file-dep/1.0.0'))).toHaveLength(1)
})
