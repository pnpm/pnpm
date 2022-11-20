/// <reference path="../../../__typings__/index.d.ts"/>
import { resolveFromTarball } from '@pnpm/tarball-resolver'

test('tarball from npm registry', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: '@registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedPref: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL that contain port number', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: '@buildserver.mycompany.com+81/my-private-package-0.1.6.tgz',
    normalizedPref: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    resolution: {
      tarball: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball not from npm registry', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'https://github.com/hegemonic/taffydb/tarball/master' })

  expect(resolutionResult).toStrictEqual({
    id: '@github.com/hegemonic/taffydb/tarball/master',
    normalizedPref: 'https://github.com/hegemonic/taffydb/tarball/master',
    resolution: {
      tarball: 'https://github.com/hegemonic/taffydb/tarball/master',
    },
    resolvedVia: 'url',
  })
})

test('tarballs from GitHub (is-negative)', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz' })

  expect(resolutionResult).toStrictEqual({
    id: '@github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    normalizedPref: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    resolution: {
      tarball: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    },
    resolvedVia: 'url',
  })
})

test('ignore direct URLs to repositories', async () => {
  expect(await resolveFromTarball({ pref: 'https://github.com/foo/bar' })).toBe(null)
  expect(await resolveFromTarball({ pref: 'https://github.com/foo/bar/' })).toBe(null)
  expect(await resolveFromTarball({ pref: 'https://gitlab.com/foo/bar' })).toBe(null)
  expect(await resolveFromTarball({ pref: 'https://bitbucket.org/foo/bar' })).toBe(null)
})
