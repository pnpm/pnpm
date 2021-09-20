/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { createCafsStore } from '@pnpm/package-store'
import createFetcher from '@pnpm/git-fetcher'
import { DependencyManifest } from '@pnpm/types'
import pDefer from 'p-defer'
import tempy from 'tempy'

test('fetch', async () => {
  const cafsDir = tempy.directory()
  const fetch = createFetcher().git
  const manifest = pDefer<DependencyManifest>()
  const { filesIndex } = await fetch(
    createCafsStore(cafsDir),
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

test('fetch a package from Git that has a prepare script', async () => {
  const cafsDir = tempy.directory()
  const fetch = createFetcher().git
  const manifest = pDefer<DependencyManifest>()
  const { filesIndex } = await fetch(
    createCafsStore(cafsDir),
    {
      commit: 'd2916cab494f6cddc85c921ffa3befb600e00e0e',
      repo: 'https://github.com/pnpm/test-git-fetch.git',
      type: 'git',
    },
    {
      manifest,
    }
  )
  expect(filesIndex[`dist${path.sep}index.js`]).toBeTruthy()
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('fetch a package without a package.json', async () => {
  const cafsDir = tempy.directory()
  const fetch = createFetcher().git
  const manifest = pDefer<DependencyManifest>()
  const { filesIndex } = await fetch(
    createCafsStore(cafsDir),
    {
      // a small Deno library with a 'denolib.json' instead of a 'package.json'
      commit: 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b',
      repo: 'https://github.com/denolib/camelcase.git',
      type: 'git',
    },
    {
      manifest,
    }
  )
  expect(filesIndex['denolib.json']).toBeTruthy()
})