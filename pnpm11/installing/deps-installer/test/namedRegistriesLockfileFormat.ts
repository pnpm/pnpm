import { expect, test } from '@jest/globals'
import { LOCKFILE_VERSION, NAMED_REGISTRIES_LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { install } from '@pnpm/installing.deps-installer'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from './utils/index.js'

const NAMED_REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const namedRegistries = { work: NAMED_REGISTRY }

function readLockfile (): LockfileFile {
  return readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
}

function installFoo (opts: { namedRegistriesLockfileFormat?: boolean }): Promise<unknown> {
  return install({
    dependencies: {
      '@pnpm.e2e/foo': 'work:1.0.0',
    },
  }, testDefaults({
    namedRegistries,
    ...opts,
  }, { namedRegistries }))
}

test('a named-registry dependency is keyed registry-qualified and stamps lockfile 9.1', async () => {
  prepareEmpty()

  await installFoo({ namedRegistriesLockfileFormat: true })

  const lockfile = readLockfile()
  expect(lockfile.lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)
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

test('the 9.1 format is opt-in: without the setting the legacy key is written', async () => {
  prepareEmpty()

  await installFoo({})

  const lockfile = readLockfile()
  expect(lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
  expect(Object.keys(lockfile.packages ?? {})).toContain('@pnpm.e2e/foo@1.0.0')
})

test('a lockfile already on 9.1 keeps the format even when the setting is off', async () => {
  prepareEmpty()

  await installFoo({ namedRegistriesLockfileFormat: true })
  expect(readLockfile().lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)

  // A teammate on a client that defaults the setting off must not rewrite the
  // lockfile back to the legacy shape.
  await installFoo({})

  const lockfile = readLockfile()
  expect(lockfile.lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)
  expect(Object.keys(lockfile.packages ?? {})).toContain('@pnpm.e2e/foo@work:1.0.0')
})

test('the same package resolved from two registries gets one entry per registry', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/foo': '1.0.0',
      'foo-from-work': 'work:@pnpm.e2e/foo@1.0.0',
    },
  }, testDefaults({
    namedRegistries,
    namedRegistriesLockfileFormat: true,
  }, { namedRegistries }))

  const lockfile = readLockfile()
  const packageKeys = Object.keys(lockfile.packages ?? {})
  // Before format 9.1 these two collapsed onto a single `@pnpm.e2e/foo@1.0.0`
  // entry and one of the two consumers silently got the other's tarball.
  expect(packageKeys).toContain('@pnpm.e2e/foo@1.0.0')
  expect(packageKeys).toContain('@pnpm.e2e/foo@work:1.0.0')
  expect(lockfile.importers?.['.']?.dependencies?.['foo-from-work']).toStrictEqual({
    specifier: 'work:@pnpm.e2e/foo@1.0.0',
    version: '@pnpm.e2e/foo@work:1.0.0',
  })
})

test('enabling the setting migrates an existing 9.0 lockfile', async () => {
  prepareEmpty()

  await installFoo({})
  expect(readLockfile().lockfileVersion).toBe(LOCKFILE_VERSION)

  // The project is otherwise up to date, and 9.0 is still a supported
  // version, so nothing else would force the re-resolution that applies the
  // format. Turning the setting on has to be enough on its own.
  await installFoo({ namedRegistriesLockfileFormat: true })

  const lockfile = readLockfile()
  expect(lockfile.lockfileVersion).toBe(NAMED_REGISTRIES_LOCKFILE_VERSION)
  expect(Object.keys(lockfile.packages ?? {})).toContain('@pnpm.e2e/foo@work:1.0.0')
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
    namedRegistries,
    namedRegistriesLockfileFormat: true,
    dedupePeers,
  }, { namedRegistries }))

  const snapshotKeys = Object.keys(readLockfile().snapshots ?? {})
  const abcKey = snapshotKeys.find((key) => key.startsWith('@pnpm.e2e/abc@'))

  // Under `dedupePeers` the suffix is rendered as `name@version` rather than
  // the peer's whole depPath. That version still has to carry the registry:
  // without it, the same name and version from two registries yield one
  // suffix, so two variants of `abc` bound to different peer artifacts would
  // collapse onto a single depPath.
  expect(abcKey).toContain('@pnpm.e2e/peer-a@work:1.0.0')
})
