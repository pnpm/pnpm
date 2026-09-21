import { expect, test } from '@jest/globals'
import { createVersionSpecFromResolvedVersion, filterDependenciesByType, guessDependencyType, updateProjectManifestObject } from '@pnpm/pkg-manifest.utils'

test('createVersionSpecFromResolvedVersion() keeps the explicit equals operator of an exact pin', () => {
  expect(createVersionSpecFromResolvedVersion('3.5.2', 'exact')).toBe('=3.5.2')
  expect(createVersionSpecFromResolvedVersion('3.5.2', 'patch')).toBe('3.5.2')
})
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
      rangeSpecStyle: 'minor',
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
      rangeSpecStyle: 'minor',
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

test('update existing peerDependencies version range', async () => {
  const manifest = await updateProjectManifestObject('/project', {
    peerDependencies: {
      foo: '^1.0.0',
    },
  }, [
    {
      alias: 'foo',
      bareSpecifier: '^2.0.0',
      resolvedVersion: '2.0.0',
    },
  ])

  expect(manifest.peerDependencies).toStrictEqual({
    foo: '^2.0.0',
  })
  expect(manifest.dependencies).toBeUndefined()
})

test('filterDependenciesByType includes peerDependencies when enabled', () => {
  const manifest = {
    dependencies: { a: '1.0.0' },
    devDependencies: { b: '2.0.0' },
    peerDependencies: { c: '^3.0.0' },
  }
  const result = filterDependenciesByType(manifest, {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: false,
    peerDependencies: true,
  })
  expect(result).toStrictEqual({ a: '1.0.0', c: '^3.0.0' })
})

test('filterDependenciesByType excludes peerDependencies by default', () => {
  const manifest = {
    dependencies: { a: '1.0.0' },
    peerDependencies: { c: '^3.0.0' },
  }
  const result = filterDependenciesByType(manifest, {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  })
  expect(result).toStrictEqual({ a: '1.0.0' })
})

test('peer dependencies respect pinned version "patch" and "none"', async () => {
  const cases = [
    { rangeSpecStyle: 'patch' as const, expected: '3.2.1' },
    { rangeSpecStyle: 'none' as const, expected: '^3.2.1' },
  ]

  await Promise.all(cases.map(async ({ rangeSpecStyle, expected }) => {
    const manifest = await updateProjectManifestObject('/project', {}, [
      {
        alias: 'foo',
        bareSpecifier: 'https://github.com/kevva/is-negative',
        resolvedVersion: '3.2.1',
        rangeSpecStyle,
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

test('peer updates prefer peerDependencies when a normal dependency has the same alias', async () => {
  const manifest = await updateProjectManifestObject('/project', {
    dependencies: { foo: '^1.0.0' },
    peerDependencies: { foo: '^1.0.0' },
  }, [{
    alias: 'foo',
    bareSpecifier: '^2.0.0',
    peer: true,
  }])

  expect(manifest.dependencies).toStrictEqual({ foo: '^1.0.0' })
  expect(manifest.peerDependencies).toStrictEqual({ foo: '^2.0.0' })
})

test('peer updates preserve npm aliases', async () => {
  const manifest = await updateProjectManifestObject('/project', {
    peerDependencies: { foo: 'npm:bar@^1.0.0' },
  }, [{
    alias: 'foo',
    bareSpecifier: 'npm:bar@*',
    resolvedVersion: '2.0.0',
    peer: true,
  }])

  expect(manifest.peerDependencies).toStrictEqual({ foo: 'npm:bar@*' })
})
