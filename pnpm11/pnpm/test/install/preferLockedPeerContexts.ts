import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { createPeerDepGraphHash } from '@pnpm/deps.path'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// @pnpm.e2e/abc-parent-with-ab transitively peer-depends on @pnpm.e2e/peer-c. It
// ends up with two compatible contexts: one resolved through
// @pnpm.e2e/abc-grand-parent-with-c (which supplies peer-c@1.0.0) and one from
// the root (which supplies peer-c@2.0.0). A later writable install must preserve
// both recorded contexts instead of collapsing them onto a single one.
test('compatible existing peer contexts survive writable lockfile regeneration', async () => {
  await addDistTag({ package: '@pnpm.e2e/abc-parent-with-ab', version: '1.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/peer-c', version: '1.0.0', distTag: 'latest' })
  prepare()

  writeYamlFileSync('pnpm-workspace.yaml', {
    strictPeerDependencies: false,
  })

  await execPnpm(['add', '@pnpm.e2e/abc-grand-parent-with-c@1.0.0', '@pnpm.e2e/peer-c@2.0.0'])
  await execPnpm(['add', '@pnpm.e2e/abc-parent-with-ab'])
  await execPnpm(['add', 'is-positive@1.0.0'])

  const lockfile = readYamlFileSync<LockfileFile>(path.resolve(WANTED_LOCKFILE))
  const snapshots = Object.keys(lockfile.snapshots ?? {})
  expect(snapshots).toContain(`@pnpm.e2e/abc-parent-with-ab@1.0.0${createPeerDepGraphHash([{ name: '@pnpm.e2e/peer-c', version: '1.0.0' }])}`)
  expect(snapshots).toContain(`@pnpm.e2e/abc-parent-with-ab@1.0.0${createPeerDepGraphHash([{ name: '@pnpm.e2e/peer-c', version: '2.0.0' }])}`)
})

test('a manifest edit does not propagate an unrelated optional peer context', async () => {
  const aManifest = {
    name: 'a',
    version: '1.0.0',
    private: true,
    dependencies: {
      'agent-base': '6.0.2',
      b: 'workspace:*',
      'tiny-invariant': '1.3.3',
    },
  }
  preparePackages([
    aManifest,
    {
      name: 'b',
      version: '1.0.0',
      private: true,
      dependencies: {
        debug: '4.4.3',
        'supports-color': '5.5.0',
      },
    },
  ])
  fs.writeFileSync('package.json', JSON.stringify({ private: true }))
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['a', 'b'],
  })

  await execPnpm(['install'])
  const initialLockfile = readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)

  const editedManifest = structuredClone(aManifest)
  Reflect.deleteProperty(editedManifest.dependencies, 'tiny-invariant')
  fs.writeFileSync(path.join('a', 'package.json'), JSON.stringify(editedManifest))
  await execPnpm(['install'])

  const installedLockfileText = fs.readFileSync(WANTED_LOCKFILE, 'utf8')
  const installedLockfile = readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
  delete initialLockfile.importers!.a.dependencies!['tiny-invariant']
  delete initialLockfile.packages!['tiny-invariant@1.3.3']
  delete initialLockfile.snapshots!['tiny-invariant@1.3.3']
  expect(installedLockfile).toEqual(initialLockfile)

  await execPnpm(['dedupe'])
  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).toBe(installedLockfileText)
})
