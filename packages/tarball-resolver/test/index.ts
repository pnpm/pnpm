/// <reference path="../../../typings/index.d.ts"/>
import resolveFromTarball from '@pnpm/tarball-resolver'
import test = require('tape')

test('tarball from npm registry', async t => {
  const resolutionResult = await resolveFromTarball({ pref: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz' })

  t.deepEqual(resolutionResult, {
    id: '@registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    normalizedPref: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    resolution: {
      tarball: 'http://registry.npmjs.org/is-array/-/is-array-1.0.1.tgz',
    },
    resolvedVia: 'url',
  })

  t.end()
})

test('tarball not from npm registry', async t => {
  const resolutionResult = await resolveFromTarball({ pref: 'https://github.com/hegemonic/taffydb/tarball/master' })

  t.deepEqual(resolutionResult, {
    id: '@github.com/hegemonic/taffydb/tarball/master',
    normalizedPref: 'https://github.com/hegemonic/taffydb/tarball/master',
    resolution: {
      tarball: 'https://github.com/hegemonic/taffydb/tarball/master',
    },
    resolvedVia: 'url',
  })

  t.end()
})

test('tarballs from GitHub (is-negative)', async t => {
  const resolutionResult = await resolveFromTarball({ pref: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz' })

  t.deepEqual(resolutionResult, {
    id: '@github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    normalizedPref: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    resolution: {
      tarball: 'https://github.com/kevva/is-negative/archive/1d7e288222b53a0cab90a331f1865220ec29560c.tar.gz',
    },
    resolvedVia: 'url',
  })

  t.end()
})

test('ignore direct URLs to repositories', async t => {
  t.equal(await resolveFromTarball({ pref: 'https://github.com/foo/bar' }), null)
  t.equal(await resolveFromTarball({ pref: 'https://github.com/foo/bar/' }), null)
  t.equal(await resolveFromTarball({ pref: 'https://gitlab.com/foo/bar' }), null)
  t.equal(await resolveFromTarball({ pref: 'https://bitbucket.org/foo/bar' }), null)
  t.end()
})
