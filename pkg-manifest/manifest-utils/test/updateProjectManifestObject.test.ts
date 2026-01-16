import { guessDependencyType, updateProjectManifestObject } from '@pnpm/manifest-utils'

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
