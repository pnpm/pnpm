/// <reference path="../../../__typings__/index.d.ts"/>
// cspell:ignore buildserver
import { resolveFromTarball as _resolveFromTarball } from '@pnpm/tarball-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'

const fetch = createFetchFromRegistry({})
const resolveFromTarball = _resolveFromTarball.bind(null, fetch)

test('tarball from npm registry', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedPref: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL that contain port number', async () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fetch: any = async (url: string) => ({ url })
  const resolutionResult = await _resolveFromTarball(fetch, { pref: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
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
    id: 'https://codeload.github.com/hegemonic/taffydb/legacy.tar.gz/refs/heads/master',
    normalizedPref: 'https://codeload.github.com/hegemonic/taffydb/legacy.tar.gz/refs/heads/master',
    resolution: {
      tarball: 'https://codeload.github.com/hegemonic/taffydb/legacy.tar.gz/refs/heads/master',
    },
    resolvedVia: 'url',
  })
})

test('tarballs from GitHub (is-negative)', async () => {
  const resolutionResult = await resolveFromTarball({ pref: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
    normalizedPref: 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
    resolution: {
      tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
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

test('ignore slash in hash', async () => {
  // expect resolve from git.
  let hash = 'path:/packages/simple-react-app'
  expect(await resolveFromTarball({ pref: `RexSkz/test-git-subdir-fetch#${hash}` })).toBe(null)
  expect(await resolveFromTarball({ pref: `RexSkz/test-git-subdir-fetch#${encodeURIComponent(hash)}` })).toBe(null)
  hash = 'heads/canary'
  expect(await resolveFromTarball({ pref: `zkochan/is-negative#${hash}` })).toBe(null)
  expect(await resolveFromTarball({ pref: `zkochan/is-negative#${encodeURIComponent(hash)}` })).toBe(null)
})
