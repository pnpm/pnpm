/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createGitFetcher } from '@pnpm/git-fetcher'
import { globalWarn } from '@pnpm/logger'
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

jest.mock('@pnpm/logger', () => {
  const originalModule = jest.requireActual('@pnpm/logger')
  return {
    ...originalModule,
    globalWarn: jest.fn(),
  }
})

beforeEach(() => {
  ;(execa as jest.Mock).mockClear()
  ;(globalWarn as jest.Mock).mockClear()
})

test('fetch', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex, manifest } = await fetch(
    createCafsStore(storeDir),
    {
      commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
      repo: 'https://github.com/kevva/is-positive.git',
      type: 'git',
    },
    {
      readManifest: true,
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesIndex['package.json']).toBeTruthy()
  expect(manifest?.name).toEqual('is-positive')
})

test('fetch a package from Git sub folder', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex } = await fetch(
    createCafsStore(storeDir),
    {
      commit: '2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22',
      repo: 'https://github.com/RexSkz/test-git-subfolder-fetch.git',
      path: '/packages/simple-react-app',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesIndex['public/index.html']).toBeTruthy()
})

test('prevent directory traversal attack when using Git sub folder', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const repo = 'https://github.com/RexSkz/test-git-subfolder-fetch.git'
  const pkgDir = '../../etc'
  await expect(
    fetch(
      createCafsStore(storeDir),
      {
        commit: '2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22',
        repo,
        path: pkgDir,
        type: 'git',
      },
      {
        filesIndexFile: path.join(storeDir, 'index.json'),
      }
    )
  ).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${repo}": Path "${pkgDir}" should be a sub directory`)
})

test('prevent directory traversal attack when using Git sub folder', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const repo = 'https://github.com/RexSkz/test-git-subfolder-fetch.git'
  const pkgDir = 'not/exists'
  await expect(
    fetch(
      createCafsStore(storeDir),
      {
        commit: '2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22',
        repo,
        path: pkgDir,
        type: 'git',
      },
      {
        filesIndexFile: path.join(storeDir, 'index.json'),
      }
    )
  ).rejects.toThrow(`Failed to prepare git-hosted package fetched from "${repo}": Path "${pkgDir}" is not a directory`)
})

test('fetch a package from Git that has a prepare script', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex } = await fetch(
    createCafsStore(storeDir),
    {
      commit: '8b333f12d5357f4f25a654c305c826294cb073bf',
      repo: 'https://github.com/pnpm/test-git-fetch.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesIndex['dist/index.js']).toBeTruthy()
})

// Test case for https://github.com/pnpm/pnpm/issues/1866
test('fetch a package without a package.json', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex } = await fetch(
    createCafsStore(storeDir),
    {
      // a small Deno library with a 'denolib.json' instead of a 'package.json'
      commit: 'aeb6b15f9c9957c8fa56f9731e914c4d8a6d2f2b',
      repo: 'https://github.com/denolib/camelcase.git',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(filesIndex['denolib.json']).toBeTruthy()
})

// Covers the regression reported in https://github.com/pnpm/pnpm/issues/4064
test('fetch a big repository', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex } = await fetch(createCafsStore(storeDir),
    {
      commit: 'a65fbf5a90f53c9d72fed4daaca59da50f074355',
      repo: 'https://github.com/sveltejs/action-deploy-docs.git',
      type: 'git',
    }, {
      filesIndexFile: path.join(storeDir, 'index.json'),
    })
  expect(filesIndex).toBeTruthy()
})

test('still able to shallow fetch for allowed hosts', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ gitShallowHosts: ['github.com'], rawConfig: {} }).git
  const resolution = {
    commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
    repo: 'https://github.com/kevva/is-positive.git',
    type: 'git' as const,
  }
  const { filesIndex, manifest } = await fetch(createCafsStore(storeDir), resolution, {
    readManifest: true,
    filesIndexFile: path.join(storeDir, 'index.json'),
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
  expect(manifest?.name).toEqual('is-positive')
})

test('fail when preparing a git-hosted package', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  await expect(
    fetch(createCafsStore(storeDir),
      {
        commit: 'ba58874aae1210a777eb309dd01a9fdacc7e54e7',
        repo: 'https://github.com/pnpm-e2e/prepare-script-fails.git',
        type: 'git',
      }, {
        filesIndexFile: path.join(storeDir, 'index.json'),
      })
  ).rejects.toThrow('Failed to prepare git-hosted package fetched from "https://github.com/pnpm-e2e/prepare-script-fails.git": @pnpm.e2e/prepare-script-fails@1.0.0 npm-install: `npm install`')
})

test('do not build the package when scripts are ignored', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ ignoreScripts: true, rawConfig: {} }).git
  const { filesIndex } = await fetch(createCafsStore(storeDir),
    {
      commit: '55416a9c468806a935636c0ad0371a14a64df8c9',
      repo: 'https://github.com/pnpm-e2e/prepare-script-works.git',
      type: 'git',
    }, {
      filesIndexFile: path.join(storeDir, 'index.json'),
    })
  expect(filesIndex['package.json']).toBeTruthy()
  expect(filesIndex['prepare.txt']).toBeFalsy()
  expect(globalWarn).toHaveBeenCalledWith('The git-hosted package fetched from "https://github.com/pnpm-e2e/prepare-script-works.git" has to be built but the build scripts were ignored.')
})

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

test('fetch only the included files', async () => {
  const storeDir = tempy.directory()
  const fetch = createGitFetcher({ rawConfig: {} }).git
  const { filesIndex } = await fetch(
    createCafsStore(storeDir),
    {
      commit: '958d6d487217512bb154d02836e9b5b922a600d8',
      repo: 'https://github.com/pnpm-e2e/pkg-with-ignored-files',
      type: 'git',
    },
    {
      filesIndexFile: path.join(storeDir, 'index.json'),
    }
  )
  expect(Object.keys(filesIndex).sort()).toStrictEqual([
    'README.md',
    'dist/index.js',
    'package.json',
  ])
})
