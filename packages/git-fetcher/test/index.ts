/// <reference path="../../../typings/index.d.ts"/>
import createCafs from '@pnpm/cafs'
import createFetcher from '@pnpm/git-fetcher'
import { DependencyManifest } from '@pnpm/types'
import pDefer = require('p-defer')
import test = require('tape')
import tempy = require('tempy')

test('fetch', async t => {
  const cafsDir = tempy.directory()
  t.comment(`cafs at ${cafsDir}`)
  const fetch = createFetcher().git
  const manifest = pDefer<DependencyManifest>()
  const { filesIndex } = await fetch(
    createCafs(cafsDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'https://github.com/kevva/is-positive.git',
      type: 'git',
    },
    {
      manifest,
    }
  )
  t.ok(filesIndex['package.json'])
  t.ok(await filesIndex['package.json'].generatingIntegrity)
  t.equal((await manifest.promise).name, 'is-positive')
  t.end()
})
