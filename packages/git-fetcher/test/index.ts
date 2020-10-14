/// <reference path="../../../typings/index.d.ts"/>
import createCafs from '@pnpm/cafs'
import createFetcher from '@pnpm/git-fetcher'
import { DependencyManifest } from '@pnpm/types'
import pDefer = require('p-defer')
import tempy = require('tempy')

test('fetch', async () => {
  const cafsDir = tempy.directory()
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
  expect(filesIndex['package.json']).toBeTruthy()
  expect(filesIndex['package.json'].writeResult).toBeTruthy()
  const name = (await manifest.promise).name
  expect(name).toEqual('is-positive')
})
