/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import createResolveFromGit from '@pnpm/git-resolver'
import git from 'graceful-git'
import isWindows from 'is-windows'

const resolveFromGit = createResolveFromGit({})

test('resolveFromGit() with commit', async () => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedPref: 'github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with no commit', async () => {
  // This is repeated twice because there was a bug which caused the normalizedPref
  // to contain the commit hash on second call.
  // The issue occured because .hosted field (which is class from the 'hosted-git-info' package)
  // was mutated. A 'committish' field was added to it.
  for (let i = 0; i < 2; i++) {
    const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative' })
    expect(resolveResult).toStrictEqual({
      id: 'github.com/zkochan/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c',
      normalizedPref: 'github:zkochan/is-negative',
      resolution: {
        tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
      },
      resolvedVia: 'git-repository',
    })
  }
})

test('resolveFromGit() with branch', async () => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#canary' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/4c39fbc124cd4944ee51cb082ad49320fab58121',
    normalizedPref: 'github:zkochan/is-negative#canary',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with tag', async () => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with tag (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with strict semver', async () => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#semver:1.0.0' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedPref: 'github:zkochan/is-negative#semver:1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with strict semver (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#semver:v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#semver:v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with range semver', async () => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#semver:^1.0.0' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
    normalizedPref: 'github:zkochan/is-negative#semver:^1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() with range semver (v-prefixed tag)', async () => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#semver:<=v0.0.7' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#semver:<=v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() fails when ref not found', async () => {
  try {
    await resolveFromGit({ pref: 'zkochan/is-negative#bad-ref' })
    throw new Error('Should have failed')
  } catch (err) {
    expect(err.message).toEqual('Could not resolve bad-ref to a commit of git://github.com/zkochan/is-negative.git.')
  }
})

test('resolveFromGit() fails when semver ref not found', async () => {
  try {
    await resolveFromGit({ pref: 'zkochan/is-negative#semver:^100.0.0' })
    throw new Error('Should have failed')
  } catch (err) {
    expect(err.message).toEqual('Could not resolve ^100.0.0 to a commit of git://github.com/zkochan/is-negative.git. Available versions are: 1.0.0, 1.0.1, 2.0.0, 2.0.1, 2.0.2, 2.1.0')
  }
})

test('resolveFromGit() with commit from non-github repo', async () => {
  // TODO: make it pass on Windows
  if (isWindows()) {
    return
  }
  const localPath = process.cwd()
  const resolveResult = await resolveFromGit({ pref: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc` })
  expect(resolveResult).toStrictEqual({
    id: `${localPath}/988c61e11dc8d9ca0b5580cb15291951812549dc`,
    normalizedPref: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc`,
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
  const resolveResult = await resolveFromGit({ pref: `git+file://${localPath}` })
  expect(resolveResult).toStrictEqual({
    id: `${localPath}/${hash}`,
    normalizedPref: `git+file://${localPath}`,
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
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'bitbucket.org/pnpmjs/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz',
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with no commit', async () => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `bitbucket.org/pnpmjs/git-resolver/${hash}`,
    normalizedPref: 'bitbucket:pnpmjs/git-resolver',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with branch', async () => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#master' })
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `bitbucket.org/pnpmjs/git-resolver/${hash}`,
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#master',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with tag', async () => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#0.3.4' })
  expect(resolveResult).toStrictEqual({
    id: 'bitbucket.org/pnpmjs/git-resolver/87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() gitlab with commit', async () => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  expect(resolveResult).toStrictEqual({
    id: 'gitlab.com/pnpm/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedPref: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() gitlab with no commit', async () => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `gitlab.com/pnpm/git-resolver/${hash}`,
    normalizedPref: 'gitlab:pnpm/git-resolver',
    resolution: {
      tarball: `https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() gitlab with branch', async () => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#master' })
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  expect(resolveResult).toStrictEqual({
    id: `gitlab.com/pnpm/git-resolver/${hash}`,
    normalizedPref: 'gitlab:pnpm/git-resolver#master',
    resolution: {
      tarball: `https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() gitlab with tag', async () => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#0.3.4' })
  expect(resolveResult).toStrictEqual({
    id: 'gitlab.com/pnpm/git-resolver/87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    normalizedPref: 'gitlab:pnpm/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url', async () => {
  const resolveResult = await resolveFromGit({ pref: 'git+ssh://git@github.com:zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form)', async () => {
  const resolveResult = await resolveFromGit({ pref: 'git+ssh://git@github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

test('resolveFromGit() normalizes full url (alternative form 2)', async () => {
  const resolveResult = await resolveFromGit({ pref: 'https://github.com/zkochan/is-negative.git#2.0.1' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
})

// This test relies on implementation detail.
// current implementation does not try git ls-remote --refs on pref with full commit hash, this fake repo url will pass.
test('resolveFromGit() private repo with commit hash', async () => {
  const resolveResult = await resolveFromGit({ pref: 'fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5' })
  expect(resolveResult).toStrictEqual({
    id: 'github.com/fake/private-repo/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    resolution: {
      commit: '2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
      repo: 'git+ssh://git@github.com/fake/private-repo.git',
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
  const resolveResult = await resolveFromGit({ pref: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git' })
  expect(resolveResult).toStrictEqual({
    id: '0000000000000000000000000000000000000000+x-oauth-basic@github.com/foo/bar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    normalizedPref: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
    resolution: {
      commit: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      repo: 'https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
})
