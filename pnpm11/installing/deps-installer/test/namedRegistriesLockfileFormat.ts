import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { install } from '@pnpm/installing.deps-installer'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from './utils/index.js'

const NAMED_REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const registriesByPrefix = { work: NAMED_REGISTRY }

function readLockfile (): LockfileFile {
  return readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
}

function installFoo (): Promise<unknown> {
  return install({
    dependencies: {
      '@pnpm.e2e/foo': 'work:1.0.0',
    },
  }, testDefaults({ registriesByPrefix }, { registriesByPrefix }))
}

test('a named-registry dependency is keyed registry-qualified', async () => {
  prepareEmpty()

  await installFoo()

  const lockfile = readLockfile()
  expect(lockfile.importers?.['.']?.dependencies?.['@pnpm.e2e/foo']).toStrictEqual({
    specifier: 'work:1.0.0',
    version: 'work:1.0.0',
  })
  expect(Object.keys(lockfile.packages ?? {})).toContain('@pnpm.e2e/foo@work:1.0.0')
  expect(Object.keys(lockfile.snapshots ?? {})).toContain('@pnpm.e2e/foo@work:1.0.0')

  // The mock registry serves canonical tarball URLs, so the entry keeps only
  // its integrity — the URL is rebuilt from the `work` alias on read.
  expect(lockfile.packages?.['@pnpm.e2e/foo@work:1.0.0']).toStrictEqual({
    resolution: { integrity: expect.any(String) },
  })
})

test('recording a registry-qualified key leaves the lockfile version alone', async () => {
  prepareEmpty()

  await installFoo()

  // The key is additive. Readers gate on the lockfile's major version, so
  // moving it would lock out every client that has not learned the new one —
  // including engines embedded in other tools, which cannot be patched.
  expect(readLockfile().lockfileVersion).toBe(LOCKFILE_VERSION)
})

test('the same package resolved from two registries gets one entry per registry', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/foo': '1.0.0',
      'foo-from-work': 'work:@pnpm.e2e/foo@1.0.0',
    },
  }, testDefaults({ registriesByPrefix }, { registriesByPrefix }))

  const lockfile = readLockfile()
  const packageKeys = Object.keys(lockfile.packages ?? {})
  // Default and named registry resolutions for the same package and version
  // require separate lockfile entries.
  expect(packageKeys).toContain('@pnpm.e2e/foo@1.0.0')
  expect(packageKeys).toContain('@pnpm.e2e/foo@work:1.0.0')
  expect(lockfile.importers?.['.']?.dependencies?.['foo-from-work']).toStrictEqual({
    specifier: 'work:@pnpm.e2e/foo@1.0.0',
    version: '@pnpm.e2e/foo@work:1.0.0',
  })
})

test('a repeat install reuses the registry-qualified entry instead of rewriting it', async () => {
  prepareEmpty()

  await installFoo()
  const first = readLockfile()

  await installFoo()

  expect(readLockfile()).toStrictEqual(first)
})

test.each([
  ['dedupePeers off', false],
  ['dedupePeers on', true],
])('a named-registry peer keeps its registry in the peer suffix (%s)', async (_label, dedupePeers) => {
  prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/abc': '1.0.0',
      '@pnpm.e2e/peer-a': 'work:1.0.0',
      '@pnpm.e2e/peer-b': '1.0.0',
      '@pnpm.e2e/peer-c': '1.0.0',
    },
  }, testDefaults({
    registriesByPrefix,
    dedupePeers,
  }, { registriesByPrefix }))

  const snapshotKeys = Object.keys(readLockfile().snapshots ?? {})
  const abcKey = snapshotKeys.find((key) => key.startsWith('@pnpm.e2e/abc@'))

  // Under `dedupePeers` the suffix is rendered as `name@version` rather than
  // the peer's whole depPath. That version still has to carry the registry:
  // without it, the same name and version from two registries yield one
  // suffix, so two variants of `abc` bound to different peer artifacts would
  // collapse onto a single depPath.
  expect(abcKey).toContain('@pnpm.e2e/peer-a@work:1.0.0')
})
