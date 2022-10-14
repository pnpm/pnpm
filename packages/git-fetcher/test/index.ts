/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { DependencyManifest } from '@pnpm/types'
import pDefer from 'p-defer'
import tempy from 'tempy'
import execa from 'execa'

jest.mock('execa', () => {
  const originalModule = jest.requireActual('execa')
  return {
    __esModule: true,
    ...originalModule,
    default: jest.fn(originalModule.default),
  }
})

beforeEach(() => {
  (execa as jest.Mock).mockClear()
})

test('fetch', async () => {
  const cafsDir = tempy.directory()
  const fetch = createGitFetcher().git
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
  const fetch = createGitFetcher().git
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
  const fetch = createGitFetcher().git
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

// Covers the regression reported in https://github.com/pnpm/pnpm/issues/4064
test('fetch a big repository', async () => {
  const cafsDir = tempy.directory()
  const fetch = createGitFetcher().git
  const manifest = pDefer<DependencyManifest>()
  const { filesIndex } = await fetch(createCafsStore(cafsDir),
    {
      commit: 'a65fbf5a90f53c9d72fed4daaca59da50f074355',
      repo: 'https://github.com/sveltejs/action-deploy-docs.git',
      type: 'git',
    }, { manifest })
  await Promise.all(Object.values(filesIndex).map(({ writeResult }) => writeResult))
})

test('still able to shallow fetch for allowed hosts', async () => {
  const cafsDir = tempy.directory()
  const fetch = createGitFetcher({ gitShallowHosts: ['github.com'] }).git
  const manifest = pDefer<DependencyManifest>()
  const resolution = {
    commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
    repo: 'https://github.com/kevva/is-positive.git',
    type: 'git' as const,
  }
  const { filesIndex } = await fetch(createCafsStore(cafsDir), resolution, {
    manifest,
  })
  const calls = (execa as jest.Mock).mock.calls
  const expectedCalls = [
    ['git', [...prefixGitArgs(), 'init']],
    ['git', [...prefixGitArgs(), 'remote', 'add', 'origin', resolution.repo]],
    [
      'git',
      [...prefixGitArgs(), 'fetch', '--depth', '1', 'origin', resolution.commit],
    ],
  ]
  for (let i = 1; i < expectedCalls.length; i++) {
    // Discard final argument as it passes temporary directory
    expect(calls[i].slice(0, -1)).toEqual(expectedCalls[i])
  }
  expect(filesIndex['package.json']).toBeTruthy()
  expect(filesIndex['package.json'].writeResult).toBeTruthy()
  const name = (await manifest.promise).name
  expect(name).toEqual('is-positive')
})

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}
