import { expect, test } from '@jest/globals'
import { guessDependencyType, updateProjectManifestObject } from '@pnpm/pkg-manifest.utils'

test('guessDependencyType()', () => {
  expect(
    guessDependencyType('foo', {
      dependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '',
      },
    })
  ).toBe('devDependencies')

  expect(
    guessDependencyType('bar', {
      dependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
    })
  ).toBe('dependencies')
})

test('peer dependencies fall back to "*" when resolved version is unavailable (git)', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'https://github.com/kevva/is-negative',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'https://github.com/kevva/is-negative',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '*',
  })
})

test('peer dependencies fall back to "*" when resolved version is unavailable (tarball)', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'https://github.com/hegemonic/taffydb/tarball/master',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '*',
  })
})

test('peer dependencies use derived range when resolved version is available (git)', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'https://github.com/kevva/is-negative',
      resolvedVersion: '2.1.0',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'https://github.com/kevva/is-negative',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '^2.1.0',
  })
})

test('peer dependencies honor pinned version when resolved version is available (tarball)', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master',
      resolvedVersion: '1.4.0',
      pinnedVersion: 'minor',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'https://github.com/hegemonic/taffydb/tarball/master',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '~1.4.0',
  })
})

test('peer dependencies derive range from resolved version for jsr protocol', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'jsr:^0.1.0',
      resolvedVersion: '0.1.0',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'jsr:^0.1.0',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '^0.1.0',
  })
})

test('peer dependencies keep prerelease resolved version without prefix', async () => {
  const manifest = await updateProjectManifestObject('/project', {}, [
    {
      alias: 'foo',
      bareSpecifier: 'https://github.com/kevva/is-negative',
      resolvedVersion: '2.1.0-rc.1',
      pinnedVersion: 'minor',
      peer: true,
      saveType: 'devDependencies',
    },
  ])

  expect(manifest.devDependencies).toStrictEqual({
    foo: 'https://github.com/kevva/is-negative',
  })
  expect(manifest.peerDependencies).toStrictEqual({
    foo: '2.1.0-rc.1',
  })
})

test('skips prototype-polluting aliases without mutating Object.prototype', async () => {
  const protoSnapshotBefore = Object.getOwnPropertyNames(Object.prototype).sort()

  const manifest = await updateProjectManifestObject('/project', {}, [
    { alias: '__proto__', bareSpecifier: '1.0.0', saveType: 'dependencies' },
    { alias: 'constructor', bareSpecifier: '1.0.0', saveType: 'dependencies' },
    { alias: 'prototype', bareSpecifier: '1.0.0', saveType: 'dependencies' },
    { alias: 'real-pkg', bareSpecifier: '2.0.0', saveType: 'dependencies' },
  ])

  expect(manifest.dependencies).toStrictEqual({ 'real-pkg': '2.0.0' })
  expect(Object.hasOwn(manifest.dependencies!, '__proto__')).toBe(false)
  expect(Object.hasOwn(manifest.dependencies!, 'constructor')).toBe(false)
  expect(Object.hasOwn(manifest.dependencies!, 'prototype')).toBe(false)

  // Object.prototype hasn't grown a new property.
  expect(Object.getOwnPropertyNames(Object.prototype).sort()).toStrictEqual(protoSnapshotBefore)
})

test('peer dependencies respect pinned version "patch" and "none"', async () => {
  const cases = [
    { pinnedVersion: 'patch' as const, expected: '3.2.1' },
    { pinnedVersion: 'none' as const, expected: '^3.2.1' },
  ]

  await Promise.all(cases.map(async ({ pinnedVersion, expected }) => {
    const manifest = await updateProjectManifestObject('/project', {}, [
      {
        alias: 'foo',
        bareSpecifier: 'https://github.com/kevva/is-negative',
        resolvedVersion: '3.2.1',
        pinnedVersion,
        peer: true,
        saveType: 'devDependencies',
      },
    ])

    expect(manifest.devDependencies).toStrictEqual({
      foo: 'https://github.com/kevva/is-negative',
    })
    expect(manifest.peerDependencies).toStrictEqual({
      foo: expected,
    })
  }))
})
