/// <reference path="../../../__typings__/index.d.ts"/>
// cspell:ignore buildserver
import { resolveFromTarball as _resolveFromTarball } from '@pnpm/tarball-resolver'
import { createFetchFromRegistry } from '@pnpm/fetch'

const fetch = createFetchFromRegistry({})
const resolveFromTarball = _resolveFromTarball.bind(null, fetch)

test('tarball from npm registry (immutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedBareSpecifier: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'https://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})
test('tarball from npm.jsr.io registry (immutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'http://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    normalizedBareSpecifier: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    resolution: {
      tarball: 'https://npm.jsr.io/~/11/@jsr/luca__flag/1.0.1.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball from URL that contain port number', async () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const fetch: any = async (url: string) => ({ url })
  const resolutionResult = await _resolveFromTarball(fetch, { bareSpecifier: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz' })

  expect(resolutionResult).toStrictEqual({
    id: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    normalizedBareSpecifier: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    resolution: {
      tarball: 'http://buildserver.mycompany.com:81/my-private-package-0.1.6.tgz',
    },
    resolvedVia: 'url',
  })
})

test('tarball not from npm registry (mutable)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://github.com/hegemonic/taffydb/tarball/master',
    normalizedBareSpecifier: 'https://github.com/hegemonic/taffydb/tarball/master',
    resolution: {
      tarball: 'https://github.com/hegemonic/taffydb/tarball/master',
    },
    resolvedVia: 'url',
  })
})

test('tarballs from GitHub (is-negative)', async () => {
  const resolutionResult = await resolveFromTarball({ bareSpecifier: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz' })

  expect(resolutionResult).toStrictEqual({
    id: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    normalizedBareSpecifier: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    resolution: {
      tarball: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    },
    resolvedVia: 'url',
  })
})

test('ignore direct URLs to repositories', async () => {
  expect(await resolveFromTarball({ bareSpecifier: 'https://github.com/foo/bar' })).toBeNull()
  expect(await resolveFromTarball({ bareSpecifier: 'https://github.com/foo/bar/' })).toBeNull()
  expect(await resolveFromTarball({ bareSpecifier: 'https://gitlab.com/foo/bar' })).toBeNull()
  expect(await resolveFromTarball({ bareSpecifier: 'https://bitbucket.org/foo/bar' })).toBeNull()
})

test('ignore slash in hash', async () => {
  // expect resolve from git.
  let hash = 'path:/packages/simple-react-app'
  expect(await resolveFromTarball({ bareSpecifier: `RexSkz/test-git-subdir-fetch#${hash}` })).toBeNull()
  expect(await resolveFromTarball({ bareSpecifier: `RexSkz/test-git-subdir-fetch#${encodeURIComponent(hash)}` })).toBeNull()
  hash = 'heads/canary'
  expect(await resolveFromTarball({ bareSpecifier: `zkochan/is-negative#${hash}` })).toBeNull()
  expect(await resolveFromTarball({ bareSpecifier: `zkochan/is-negative#${encodeURIComponent(hash)}` })).toBeNull()
})
