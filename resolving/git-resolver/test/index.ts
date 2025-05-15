/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { createGitResolver } from '@pnpm/git-resolver'
import git from 'graceful-git'
import isWindows from 'is-windows'
import { fetchWithAgent } from '@pnpm/fetch'

const resolveFromGit = createGitResolver({})

function mockFetchAsPrivate (): void {
  type FetchWithAgent = typeof fetchWithAgent
  type MockedFetchWithAgent = jest.MockedFunction<FetchWithAgent>
  (fetchWithAgent as MockedFetchWithAgent).mockImplementation(async (_url, _opts) => {
    return { ok: false } as any // eslint-disable-line @typescript-eslint/no-explicit-any
  })
}

test('resolveFromGit() with commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedBareSpecifier: 'github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
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
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with partial commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zoli-forks/cmd-shim#a00a83a' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a',
    normalizedBareSpecifier: 'github:zoli-forks/cmd-shim#a00a83a',
    resolution: {
      tarball: 'https://codeload.github.com/zoli-forks/cmd-shim/tar.gz/a00a83a',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with branch', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#canary' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
    normalizedBareSpecifier: 'github:zkochan/is-negative#canary',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
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
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with tag', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
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
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with range semver', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'zkochan/is-negative#semver:^1.0.0' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
    normalizedBareSpecifier: 'github:zkochan/is-negative#semver:^1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
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
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with sub folder', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'github:RexSkz/test-git-subfolder-fetch.git#path:/packages/simple-react-app' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22#path:/packages/simple-react-app',
    normalizedBareSpecifier: 'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app',
    resolution: {
      tarball: 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/2b42a57a945f19f8ffab8ecbd2021fdc2c58ee22',
      path: '/packages/simple-react-app',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with both sub folder and branch', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'github:RexSkz/test-git-subfolder-fetch.git#beta&path:/packages/simple-react-app' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/777e8a3e78cc89bbf41fb3fd9f6cf922d5463313#path:/packages/simple-react-app',
    normalizedBareSpecifier: 'github:RexSkz/test-git-subfolder-fetch#beta&path:/packages/simple-react-app',
    resolution: {
      tarball: 'https://codeload.github.com/RexSkz/test-git-subfolder-fetch/tar.gz/777e8a3e78cc89bbf41fb3fd9f6cf922d5463313',
      path: '/packages/simple-react-app',
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
  const result = await git(['rev-parse', 'origin/master'], { retries: 0 })
  const hash: string = result.stdout.trim()
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
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with no commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'bitbucket:pnpmjs/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
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
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
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

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedBareSpecifier: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
    },
    resolvedVia: 'git-repository',
  })
})

// This test stopped working. Probably an environmental issue.
test.skip('resolveFromGit() gitlab with no commit', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'gitlab:pnpm/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
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
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
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
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com:zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url with port', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com:22:zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+ssh://git@github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form 2)', async () => {
  const resolveResult = await resolveFromGit({ bareSpecifier: 'https://github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

// This test relies on implementation detail.
// current implementation does not try git ls-remote --refs on bareSpecifier with full commit hash, this fake repo url will pass.
test('resolveFromGit() private repo with commit hash', async () => {
  mockFetchAsPrivate()
  const resolveResult = await resolveFromGit({ bareSpecifier: 'fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5' })
  expect(resolveResult).toStrictEqual({
    id: 'git+ssh://git@github.com/fake/private-repo.git#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedBareSpecifier: 'github:fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    resolution: {
      commit: '2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
      repo: 'git+ssh://git@github.com/fake/private-repo.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve a private repository using the HTTPS protocol without auth token', async () => {
  git.mockImplementation(async (args: string[]) => {
    expect(args).toContain('git+ssh://git@github.com/foo/bar.git')
    if (args.includes('--refs')) {
      return {
        stdout: `\n${'a'.repeat(40)}\trefs/heads/master\n`,
      }
    }
    return {
      stdout: '0'.repeat(40) + '\tHEAD',
    }
  })
  mockFetchAsPrivate()
  const resolveResult = await resolveFromGit({ bareSpecifier: 'git+https://github.com/foo/bar.git' })
  expect(resolveResult).toStrictEqual({
    id: 'git+ssh://git@github.com/foo/bar.git#0000000000000000000000000000000000000000',
    normalizedBareSpecifier: 'github:foo/bar',
    resolution: {
      commit: '0000000000000000000000000000000000000000',
      repo: 'git+ssh://git@github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve a private repository using the HTTPS protocol with a commit hash', async () => {
  git.mockImplementation(async (args: string[]) => {
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
    normalizedBareSpecifier: 'git+https://github.com/foo/bar.git',
    resolution: {
      // cspell:ignore aabbccddeeff
      commit: 'aabbccddeeff',
      repo: 'https://github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolve a private repository using the HTTPS protocol and an auth token', async () => {
  git.mockImplementation(async (args: string[]) => {
    if (!args.includes('https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git')) throw new Error('')
    if (args.includes('--refs')) {
      return {
        stdout: '\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/master\
',
      }
    }
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
  git.mockImplementation(async (args: string[]) => {
    if (!args.includes('ssh://git@example.com/org/repo.git')) throw new Error('')
    if (args.includes('--refs')) {
      return {
        stdout: '\
ed3de20970d980cf21a07fd8b8732c70d5182303\trefs/tags/v0.0.38\n\
cba04669e621b85fbdb33371604de1a2898e68e9\trefs/tags/v0.0.39\
',
      }
    }
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
  git.mockImplementation(async (args: string[]) => {
    if (!args.includes('ssh://git@example.com/org/repo.git')) throw new Error('')
    if (args.includes('--refs')) {
      return {
        stdout: '\
ed3de20970d980cf21a07fd8b8732c70d5182303\trefs/tags/v0.0.38\n\
cba04669e621b85fbdb33371604de1a2898e68e9\trefs/tags/v0.0.39\
',
      }
    }
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
