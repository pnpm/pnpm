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

test('writes prototype-conflicting aliases as own data properties without polluting Object.prototype', async () => {
  const protoSnapshotBefore = Object.getOwnPropertyNames(Object.prototype).sort()

  const manifest = await updateProjectManifestObject('/project', {}, [
    { alias: '__proto__', bareSpecifier: '1.0.0', saveType: 'dependencies' },
    { alias: 'constructor', bareSpecifier: '1.0.1', saveType: 'dependencies' },
    { alias: 'prototype', bareSpecifier: '1.0.2', saveType: 'dependencies' },
    { alias: 'real-pkg', bareSpecifier: '2.0.0', saveType: 'dependencies' },
  ])

  // Each pollution-key alias is stored as a regular own data property.
  const deps = manifest.dependencies!
  expect(Object.hasOwn(deps, '__proto__')).toBe(true)
  expect(Object.hasOwn(deps, 'constructor')).toBe(true)
  expect(Object.hasOwn(deps, 'prototype')).toBe(true)
  expect(Object.hasOwn(deps, 'real-pkg')).toBe(true)
  // The own __proto__ data property shadows the inherited getter and returns the value.
  expect(deps.__proto__).toBe('1.0.0')
  expect(deps.constructor as unknown as string).toBe('1.0.1')
  expect(deps.prototype as unknown as string).toBe('1.0.2')
  // The prototype chain of `deps` is unchanged (the assignment did not run __proto__'s setter).
  expect(Object.getPrototypeOf(deps)).toBe(Object.prototype)

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
