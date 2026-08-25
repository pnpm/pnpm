/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import isWindows from 'is-windows'

jest.unstable_mockModule('@pnpm/network.fetch', () => ({
  fetchWithDispatcher: jest.fn(),
}))
jest.unstable_mockModule('execa', () => ({
  safeExeca: jest.fn(),
}))
const { fetchWithDispatcher } = await import('@pnpm/network.fetch')
const { safeExeca: execa } = await import('execa')
const { createGitResolver } = await import('@pnpm/resolving.git-resolver')
const { lsRemote } = await import('../lib/lsRemote.js')

const resolveFromGit = createGitResolver({})

beforeEach(() => {
  mockGit(lsRemoteFromFixture)
  mockFetchAsPublic()
})

test('resolveFromGit() passes GIT_TERMINAL_PROMPT=0 to prevent interactive credential prompts', async () => {
  await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#master' })
  expect(jest.mocked(execa)).toHaveBeenCalledWith(
    'git',
    expect.any(Array),
    expect.objectContaining({
      env: expect.objectContaining({
        GIT_TERMINAL_PROMPT: '0',
      }),
    })
  )
})

test('resolveFromGit() with commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedBareSpecifier: 'github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with no commit', async () => {
  // This is repeated twice because there was a bug which caused the specifier
  // to contain the commit hash on second call.
  // The issue occurred because .hosted field (which is class from the 'hosted-git-info' package)
  // was mutated. A 'committish' field was added to it.
  for (let i = 0; i < 2; i++) {
    const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative' }) // eslint-disable-line no-await-in-loop
    expect(resolveResult).toStrictEqual({
      id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
      normalizedBareSpecifier: 'github:zkochan/is-negative',
      resolution: {
        tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
        gitHosted: true,
      },
      resolvedVia: 'git-repository',
    })
  }
})

test('resolveFromGit() with no commit, when main branch is not master', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zoli-forks/cmd-shim' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5',
    normalizedBareSpecifier: 'github:zoli-forks/cmd-shim',
    resolution: {
      tarball: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with partial commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zoli-forks/cmd-shim#a00a83a' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5',
    normalizedBareSpecifier: 'github:zoli-forks/cmd-shim#a00a83a',
    resolution: {
      tarball: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with partial commit that is a branch name', async () => {
  await expect(
    resolveFromGit({ bareSpecifier: 'pnpm-e2e/simple-pkg#deadbeef' })
  ).rejects.toThrow(/resolved commit [0-9a-f]{40} from commit-ish reference deadbeef/)
})

test('resolveFromGit() with branch', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#canary' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
    normalizedBareSpecifier: 'github:zkochan/is-negative#canary',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with branch relative to refs', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#heads/canary' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
    normalizedBareSpecifier: 'github:zkochan/is-negative#heads/canary',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with tag', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test.skip('resolveFromGit() with tag (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'andreineculau/npm-publish-git#v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedBareSpecifier: 'github:andreineculau/npm-publish-git#v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with strict semver', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#semver:1.0.0' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedBareSpecifier: 'github:zkochan/is-negative#semver:1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test.skip('resolveFromGit() with strict semver (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'andreineculau/npm-publish-git#semver:v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedBareSpecifier: 'github:andreineculau/npm-publish-git#semver:v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with range semver', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#semver:^1.0.0' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/f7dec4d66a5a56719e49b9f94a24d73f924ddeb3',
    normalizedBareSpecifier: 'github:zkochan/is-negative#semver:^1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/f7dec4d66a5a56719e49b9f94a24d73f924ddeb3',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test.skip('resolveFromGit() with range semver (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'andreineculau/npm-publish-git#semver:<=v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedBareSpecifier: 'github:andreineculau/npm-publish-git#semver:<=v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with sub folder', async () => {
  const headCommit = '2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22'
  jest.mocked(fetchWithDispatcher).mockImplementation(async (_url, _opts) => {
    return { ok: true } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
  mockGit(async (args: string[]) => {
    if (args.includes('--exit-code')) {
      return { stdout: `${headCommit}\tHEAD` }
    }
    return { stdout: `${headCommit}\tHEAD` }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'github:RexSkz/test-git-subfolder-fetch.git#path:/packages/simple-react-app' })
  expect(resolveResult).toStrictEqual({
    id: `https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/${headCommit}#path:/packages/simple-react-app`,
    normalizedBareSpecifier: 'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app',
    resolution: {
      tarball: `https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/${headCommit}`,
      path: '/packages/simple-react-app',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with both sub folder and branch', async () => {
  const betaCommit = '777e8a3e78cc89bbf41fb3fd9f6cf922d5463313'
  jest.mocked(fetchWithDispatcher).mockImplementation(async (_url, _opts) => {
    return { ok: true } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
  mockGit(async (args: string[]) => {
    if (args.includes('--exit-code')) {
      return { stdout: `${betaCommit}\tHEAD` }
    }
    return { stdout: `${betaCommit}\trefs/heads/beta` }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'github:RexSkz/test-git-subfolder-fetch.git#beta&path:/packages/simple-react-app' })
  expect(resolveResult).toStrictEqual({
    id: `https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/${betaCommit}#path:/packages/simple-react-app`,
    normalizedBareSpecifier: 'github:RexSkz/test-git-subfolder-fetch#beta&path:/packages/simple-react-app',
    resolution: {
      tarball: `https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/${betaCommit}`,
      path: '/packages/simple-react-app',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() fails when ref not found', async () => {
  await expect(
    resolveFromGit({ bareSpecifier: 'zkochan/is-negative#bad-ref' })
  ).rejects.toThrow(/Could not resolve bad-ref to a commit of (https|git):\/\/github.com\/zkochan\/is-negative.git./)
})

test('resolveFromGit() fails when semver ref not found', async () => {
  await expect(
    resolveFromGit({ bareSpecifier: 'zkochan/is-negative#semver:^100.0.0' })
  ).rejects.toThrow(/Could not resolve \^100.0.0 to a commit of (https|git):\/\/github.com\/zkochan\/is-negative.git. Available versions are: 1.0.0, 1.0.1, 2.0.0, 2.0.1, 2.0.2, 2.1.0/)
})

test('resolveFromGit() with commit from non-github repo', async () => {
  // TODO: make it pass on Windows
  if (isWindows()) {
    return
  }
  const localPath = process.cwd()
  const resolveResult = await resolveFromGit({ bareSpecifier: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc` })
  expect(resolveResult).toStrictEqual({
    id: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc`,
    normalizedBareSpecifier: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc`,
    resolution: {
      commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
      repo: `file://${localPath}`,
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

// TODO: make it pass on CI servers
test.skip('resolveFromGit() with commit from non-github repo with no commit', async () => {
  const localPath = path.resolve('..', '..')
  const result = await execa('git', ['rev-parse', 'origin/master'])
  const hash = (result.stdout as string).trim()
  const resolveResult = await resolveFromGit({ bareSpecifier: `git+file://${localPath}` })
  expect(resolveResult).toStrictEqual({
    id: `git+file://${localPath}#${hash}`,
    normalizedBareSpecifier: `git+file://${localPath}`,
    resolution: {
      commit: hash,
      repo: `file://${localPath}`,
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with commit', async () => {
  // TODO: make it pass on Windows
  if (isWindows()) {
    return
  }
  const resolveResult = await resolveFromGit({ bareSpecifier: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'https://bitbucket.org/pnpmjs/git-resolver/get/988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz',
    normalizedBareSpecifier: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with no commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'bitbucket:pnpmjs/git-resolver' })
  const result = await lsRemote(['--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    normalizedBareSpecifier: 'bitbucket:pnpmjs/git-resolver',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with branch', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'bitbucket:pnpmjs/git-resolver#master' })
  const result = await lsRemote(['--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    normalizedBareSpecifier: 'bitbucket:pnpmjs/git-resolver#master',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with tag', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'bitbucket:pnpmjs/git-resolver#0.3.4' })
  expect(resolveResult).toStrictEqual({
    id: 'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
    normalizedBareSpecifier: 'bitbucket:pnpmjs/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() gitlab with colon in the URL', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'ssh://git@gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedBareSpecifier: 'ssh://git@gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
      repo: 'ssh://git@gitlab/pnpm/git-resolver',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

// Regression test for #11533: the tarball URL must not contain `%2F`,
// otherwise GitLab returns 406 and Node refuses to import the package
// (the encoded slash ends up in the virtual store directory name).
test('resolveFromGit() gitlab tarball uses /-/archive/ URL without encoded slash', async () => {
  const headCommit = '988c61e11dc8d9ca0b5580cb15291951812549dc'
  jest.mocked(fetchWithDispatcher).mockImplementation(async (_url, _opts) => {
    return { ok: true } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
  mockGit(async () => ({ stdout: `${headCommit}\tHEAD` }))
  const resolveResult = await resolveFromGit({ bareSpecifier: 'https://gitlab.com/pnpmjs/git-resolver' })
  expect(resolveResult).toStrictEqual({
    id: `https://gitlab.com/pnpmjs/git-resolver/-/archive/${headCommit}/git-resolver-${headCommit}.tar.gz`,
    normalizedBareSpecifier: 'gitlab:pnpmjs/git-resolver',
    resolution: {
      tarball: `https://gitlab.com/pnpmjs/git-resolver/-/archive/${headCommit}/git-resolver-${headCommit}.tar.gz`,
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedBareSpecifier: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with no commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver' })
  const result = await lsRemote(['--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=${hash}`,
    normalizedBareSpecifier: 'gitlab:pnpm/git-resolver',
    resolution: {
      tarball: `https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
})

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with branch', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver#master' })
  const result = await lsRemote(['--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=${hash}`,
    normalizedBareSpecifier: 'gitlab:pnpm/git-resolver#master',
    resolution: {
      tarball: `https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
})

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with tag', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver#0.3.4' })
  expect(resolveResult).toStrictEqual({
    id: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    normalizedBareSpecifier: 'gitlab:pnpm/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=87cf6a67064d2ce56e8cd20624769a5512b83ff9',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com:zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url with port', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com:22:zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form 2)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'https://github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/6dcce91c268805d456b8a575b67d7febc7ae2933',
      gitHosted: true,
    },
    resolvedVia: 'git-repository',
  })
})

// This test relies on implementation detail.
// current implementation does not try git ls-remote on bareSpecifier with full commit hash, this fake repo url will pass.
test('resolveFromGit() private repo with commit hash', async () => {
  // parseBareSpecifier will try to access the repository with --exit-code
  mockGit(() => {
    throw new Error('private')
  })
  mockFetchAsPrivate()
  const resolveResult = await resolveFromGit({ bareSpecifier: 'fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5' })
  expect(resolveResult).toStrictEqual({
    id: 'git+https://github.com/fake/private-repo.git#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    resolution: {
      commit: '2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
      repo: 'https://github.com/fake/private-repo.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve a private repository using the HTTPS protocol without auth token', async () => {
  mockGit(async (args: string[]) => {
    // Probes use --exit-code, resolution calls use --. Fail probes, succeed resolution.
    if (args.includes('--exit-code')) {
      throw new Error('access denied')
    }
    expect(args).toContain('https://github.com/foo/bar.git')
    return {
      stdout: '0'.repeat(40) + '\tHEAD',
    }
  })
  mockFetchAsPrivate()
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+https://github.com/foo/bar.git' })
  expect(resolveResult).toStrictEqual({
    id: 'git+https://github.com/foo/bar.git#0000000000000000000000000000000000000000',
    normalizedBareSpecifier: 'github:foo/bar',
    resolution: {
      commit: '0000000000000000000000000000000000000000',
      repo: 'https://github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve over HTTPS when the visibility probe fails but anonymous HTTPS git access works', async () => {
  // A public repo whose HEAD probe is throttled (e.g. GitHub rate-limiting a CI
  // runner) must still resolve over HTTPS: recording SSH would poison the
  // lockfile for every environment without SSH keys.
  mockFetchAsPrivate()
  const gitCalls: string[][] = []
  mockGit(async (args: string[]) => {
    gitCalls.push(args)
    if (args.some((arg) => arg.includes('git@'))) throw new Error('Permission denied (publickey)')
    return { stdout: '0'.repeat(40) + '\tHEAD' }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'foo/bar' })
  expect(resolveResult).toStrictEqual({
    id: `git+https://github.com/foo/bar.git#${'0'.repeat(40)}`,
    normalizedBareSpecifier: 'git+https://github.com/foo/bar.git',
    resolution: {
      commit: '0'.repeat(40),
      repo: 'https://github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
  expect(gitCalls.flat().some((arg) => arg.includes('git@'))).toBe(false)
})

test('a public repository resolves over HTTPS without probing git access', async () => {
  const gitCalls: string[][] = []
  mockGit(async (args: string[]) => {
    gitCalls.push(args)
    return lsRemoteFromFixture(args)
  })
  await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#master' })
  expect(gitCalls).toStrictEqual([
    ['ls-remote', '--', 'https://github.com/zkochan/is-negative.git', 'master', 'master^{}'],
  ])
})

test('an explicit SSH specifier resolves over SSH when git cannot use HTTPS', async () => {
  mockGit(async (args: string[]) => {
    if (args.some((arg) => arg.startsWith('https://'))) throw new Error('SSL certificate problem')
    return { stdout: '0'.repeat(40) + '\tHEAD' }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com/foo/bar.git' })
  expect(resolveResult?.resolution).toStrictEqual({
    commit: '0'.repeat(40),
    repo: 'git@github.com:foo/bar.git',
    type: 'git',
  })
})

// Covers https://github.com/pnpm/pnpm/issues/13743.
test('an unreachable remote names the dependency and how to substitute the transport', async () => {
  mockGit(async () => {
    throw Object.assign(new Error('Command failed with exit code 128'), {
      stderr: "fatal: unable to access 'https://github.com/zkochan/is-negative.git/': SSL certificate problem",
    })
  })
  const err = await resolveFailure(resolveFromGit({ bareSpecifier: 'zkochan/is-negative#next' }))
  expect(err.code).toBe('ERR_PNPM_GIT_RESOLVE_FAILED')
  expect(err.message).toBe('Failed to resolve git dependency "zkochan/is-negative#next": git ls-remote failed: fatal: unable to access \'https://github.com/zkochan/is-negative.git/\': SSL certificate problem')
  expect(err.hint).toContain('git config --global url."git@github.com:".insteadOf "https://github.com/"')
})

test('an unreachable remote redacts the credentials git echoes back', async () => {
  mockGit(async () => {
    throw Object.assign(new Error('Command failed with exit code 128'), {
      stderr: "fatal: unable to access 'https://hunter2:x-oauth-basic@github.com/foo/bar.git/': not found",
    })
  })
  const err = await resolveFailure(resolveFromGit({ bareSpecifier: 'git+https://hunter2:x-oauth-basic@github.com/foo/bar.git' }))
  expect(err.message).not.toContain('hunter2')
  expect(err.message).not.toContain('x-oauth-basic')
  expect(err.hint).not.toContain('hunter2')
})

test.each([
  ['an unresolvable ref', 'no-such-branch', 'Could not resolve no-such-branch to a commit of'],
  ['a range matching no tag', 'semver:^1.0.0', 'Could not resolve ^1.0.0 to a commit of'],
])('%s redacts the credentials the repository URL carries', async (_name, committish, expected) => {
  mockGit(async () => ({ stdout: `${'0'.repeat(40)}\tHEAD` }))
  mockFetchAsPrivate()
  const err = await resolveFailure(resolveFromGit({ bareSpecifier: `git+https://hunter2:x-oauth-basic@github.com/foo/bar.git#${committish}` }))
  expect(err.message).toContain(expected)
  // The whole `user:pass@` goes, not just one half of it — a GitHub token is
  // the *user* in `<token>:x-oauth-basic@` — while the part of the URL that
  // tells the reader which repository failed stays.
  expect(err.message).not.toContain('hunter2')
  expect(err.message).not.toContain('x-oauth-basic')
  expect(err.message).toContain('https://github.com/foo/bar.git')
})

test('a missing git binary is reported as one', async () => {
  mockGit(async () => {
    throw Object.assign(new Error('spawn git ENOENT'), { code: 'ENOENT' })
  })
  const err = await resolveFailure(resolveFromGit({ bareSpecifier: 'zkochan/is-negative#master' }))
  expect(err.message).toBe('Failed to resolve git dependency "zkochan/is-negative#master": git ls-remote failed: `git` executable not found on PATH. Install git to resolve git-hosted packages.')
})

test('an unreachable SSH remote carries no transport substitution hint', async () => {
  mockFetchAsPrivate()
  mockGit(async () => {
    throw new Error('Permission denied (publickey)')
  })
  const err = await resolveFailure(resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com/foo/bar.git' }))
  expect(err.code).toBe('ERR_PNPM_GIT_RESOLVE_FAILED')
  expect(err.hint).toBeUndefined()
})

test('resolve an explicit SSH specifier over SSH when only SSH access works', async () => {
  mockFetchAsPrivate()
  mockGit(async (args: string[]) => {
    if (!args.includes('git@github.com:foo/bar.git')) throw new Error('access denied')
    return { stdout: '0'.repeat(40) + '\tHEAD' }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com/foo/bar.git' })
  expect(resolveResult).toStrictEqual({
    id: `git+ssh://git@github.com/foo/bar.git#${'0'.repeat(40)}`,
    normalizedBareSpecifier: 'github:foo/bar',
    resolution: {
      commit: '0'.repeat(40),
      repo: 'git@github.com:foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('the terminal credential prompt is disabled when a private repository is probed and resolved', async () => {
  mockFetchAsPrivate()
  let invocations = 0
  mockGit(async () => {
    invocations++
    return { stdout: '0'.repeat(40) + '\tHEAD' }
  })
  await resolveFromGit({ bareSpecifier: 'git+https://github.com/foo/bar.git' })
  // The access probe and the ref resolution both shell out to git.
  expect(invocations).toBe(2)
})

test('resolve a private repository using the HTTPS protocol with a commit hash', async () => {
  mockFetchAsPrivate()
  mockGit(async (args: string[]) => {
    expect(args).toContain('ls-remote')
    expect(args).toContain('https://github.com/foo/bar.git')
    return {
      // cspell:ignore aabbccddeeff
      stdout: 'aabbccddeeff\tHEAD',
    }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+https://github.com/foo/bar.git#aabbccddeeff' })
  expect(resolveResult).toStrictEqual({
    id: 'git+https://github.com/foo/bar.git#aabbccddeeff',
    normalizedBareSpecifier: 'git+https://github.com/foo/bar.git#aabbccddeeff',
    resolution: {
      // cspell:ignore aabbccddeeff
      commit: 'aabbccddeeff',
      repo: 'https://github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

// [pnpm/pnpm#13999](https://github.com/pnpm/pnpm/issues/13999)
test('a private repository reached over HTTPS keeps the branch in the recorded specifier', async () => {
  mockFetchAsPrivate()
  mockGit(async (args: string[]) => {
    if (args.includes('--exit-code')) return { stdout: `${'0'.repeat(40)}\tHEAD` }
    return { stdout: `${'1'.repeat(40)}\trefs/heads/develop` }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'foo/bar#develop' })
  expect(resolveResult).toStrictEqual({
    id: `git+https://github.com/foo/bar.git#${'1'.repeat(40)}`,
    normalizedBareSpecifier: 'git+https://github.com/foo/bar.git#develop',
    resolution: {
      commit: '1'.repeat(40),
      repo: 'https://github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('every representation of a hosted specifier keeps its committish', async () => {
  mockGit(async (args: string[]) => {
    if (args.includes('--exit-code')) return { stdout: `${'0'.repeat(40)}\tHEAD` }
    return { stdout: `${'1'.repeat(40)}\trefs/heads/develop` }
  })
  const publicRepo = await resolveFromGit({ bareSpecifier: 'foo/bar#develop' })
  mockFetchAsPrivate()
  const privateRepo = await resolveFromGit({ bareSpecifier: 'foo/bar#develop' })
  const credentialedRepo = await resolveFromGit({ bareSpecifier: 'git+https://hunter2:x-oauth-basic@github.com/foo/bar.git#develop' })
  expect([
    publicRepo?.normalizedBareSpecifier,
    privateRepo?.normalizedBareSpecifier,
    credentialedRepo?.normalizedBareSpecifier,
  ]).toStrictEqual([
    'github:foo/bar#develop',
    'git+https://github.com/foo/bar.git#develop',
    'git+https://hunter2:x-oauth-basic@github.com/foo/bar.git#develop',
  ])
})

test('resolve a private repository using the HTTPS protocol and an auth token', async () => {
  mockGit(async (args: string[]) => {
    if (!args.includes('https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git')) throw new Error('')
    return { stdout: '0000000000000000000000000000000000000000\tHEAD' }
  })
  mockFetchAsPrivate()
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git' })
  expect(resolveResult).toStrictEqual({
    id: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git#0000000000000000000000000000000000000000',
    normalizedBareSpecifier: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
    resolution: {
      commit: '0000000000000000000000000000000000000000',
      repo: 'https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve an internal repository using SSH protocol with range semver', async () => {
  mockGit(async (args: string[]) => {
    if (!args.includes('ssh://git@example.com/org/repo.git')) throw new Error('')
    return {
      stdout: '0000000000000000000000000000000000000000\tHEAD\n\
ed3de20970d980cf21a07fd8b8732c70d5182303\trefs/tags/v0.0.38\n\
cba04669e621b85fbdb33371604de1a2898e68e9\trefs/tags/v0.0.39',
    }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@example.com/org/repo.git#semver:~0.0.38' })
  expect(resolveResult).toStrictEqual({
    id: 'git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9',
    normalizedBareSpecifier: 'git+ssh://git@example.com/org/repo.git#semver:~0.0.38',
    resolution: {
      commit: 'cba04669e621b85fbdb33371604de1a2898e68e9',
      repo: 'ssh://git@example.com/org/repo.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve an internal repository using SSH protocol with range semver and SCP-like URL', async () => {
  mockGit(async (args: string[]) => {
    if (!args.includes('ssh://git@example.com/org/repo.git')) throw new Error('')
    return {
      stdout: '0000000000000000000000000000000000000000\tHEAD\n\
ed3de20970d980cf21a07fd8b8732c70d5182303\trefs/tags/v0.0.38\n\
cba04669e621b85fbdb33371604de1a2898e68e9\trefs/tags/v0.0.39',
    }
  })
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@example.com:org/repo.git#semver:~0.0.38' })
  expect(resolveResult).toStrictEqual({
    id: 'git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9',
    normalizedBareSpecifier: 'git+ssh://git@example.com:org/repo.git#semver:~0.0.38',
    resolution: {
      commit: 'cba04669e621b85fbdb33371604de1a2898e68e9',
      repo: 'ssh://git@example.com/org/repo.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

function mockGit (run: (args: string[]) => Promise<{ stdout: string }>): void {
  jest.mocked(execa).mockImplementation(((file: string, args?: readonly string[], opts?: { env?: NodeJS.ProcessEnv }) => {
    // Every git invocation has to disable the terminal credential prompt,
    // otherwise a repository that needs credentials blocks the command.
    expect(file).toBe('git')
    expect(opts?.env?.GIT_TERMINAL_PROMPT).toBe('0')
    return run(args ? [...args] : [])
  }) as any) // eslint-disable-line @typescript-eslint/no-explicit-any
}

function mockFetchAsPublic (): void {
  jest.mocked(fetchWithDispatcher).mockImplementation(async (_url, _opts) => {
    return { ok: true } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
}

function mockFetchAsPrivate (): void {
  jest.mocked(fetchWithDispatcher).mockImplementation(async (_url, _opts) => {
    return { ok: false } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
}

async function resolveFailure (resolving: Promise<unknown>): Promise<PnpmError> {
  return resolving.then(
    () => {
      throw new Error('expected the git resolution to fail')
    },
    (err: unknown) => err as PnpmError
  )
}

async function lsRemoteFromFixture (args: string[]): Promise<{ stdout: string }> {
  const repo = args.find((arg) => REPO_REFS[arg] != null)
  if (args[0] !== 'ls-remote' || repo == null) {
    throw new Error(`No fixture for git command: git ${args.join(' ')}`)
  }
  return {
    stdout: REPO_REFS[repo].map(([commit, ref]) => `${commit}\t${ref}`).join('\n'),
  }
}

// Captured from `git ls-remote` against the real repositories (abridged for
// cmd-shim). The commit hashes expected by the tests come from these refs.
const REPO_REFS: Record<string, Array<[commit: string, ref: string]>> = {
  'https://github.com/zkochan/is-negative.git': [
    ['1d7e288222b53a0cab90a331f1865220ec29560c', 'HEAD'],
    ['4c39fbc124cd4944ee51cb082ad49320fab58121', 'refs/heads/canary'],
    ['1d7e288222b53a0cab90a331f1865220ec29560c', 'refs/heads/master'],
    ['163360a8d3ae6bee9524541043197ff356f8ed99', 'refs/tags/1.0.0'],
    ['9a89df745b2ec20ae7445d3d9853ceaeef5b0b72', 'refs/tags/1.0.1'],
    ['f7dec4d66a5a56719e49b9f94a24d73f924ddeb3', 'refs/tags/1.0.1^{}'],
    ['ec74951f0a5d3ba294e11a49230529e89f0ebac7', 'refs/tags/2.0.0'],
    ['219c424611ff4a2af15f7deeff4f93c62558c43d', 'refs/tags/2.0.0^{}'],
    ['2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5', 'refs/tags/2.0.1'],
    ['6dcce91c268805d456b8a575b67d7febc7ae2933', 'refs/tags/2.0.1^{}'],
    ['94cd32f6b993eebb3abe891efbec6656b4c56532', 'refs/tags/2.0.2'],
    ['2a6169d91678bdf435503a35742ca12a1af85396', 'refs/tags/2.0.2^{}'],
    ['54355c870aab5b671fc7abe261d004b326a2592d', 'refs/tags/2.1.0'],
    ['a6c51a38c6c1753e8ea1b51dfb4e6f2f8fb55557', 'refs/tags/2.1.0^{}'],
  ],
  'https://github.com/zoli-forks/cmd-shim.git': [
    ['a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5', 'HEAD'],
    ['884988ef307d9d6e5bc2b93ba013baed552d5091', 'refs/heads/fix/now-cli-issue'],
    ['a00a83a1593edb6e395d3ce41f2ef70edf7e2cf5', 'refs/heads/main'],
  ],
  'https://github.com/pnpm-e2e/simple-pkg.git': [
    ['2fce895ee534a38989bb67fdb8684f520827f614', 'HEAD'],
    ['2fce895ee534a38989bb67fdb8684f520827f614', 'refs/heads/branch/with-slash'],
    ['2fce895ee534a38989bb67fdb8684f520827f614', 'refs/heads/deadbeef'],
    ['2fce895ee534a38989bb67fdb8684f520827f614', 'refs/heads/main'],
  ],
}
