/// <reference path="../../../typings/index.d.ts"/>
import path = require('path')
import git = require('graceful-git')
import isWindows = require('is-windows')
import proxyquire = require('proxyquire')
import test = require('tape')

let gracefulGit = git
const gracefulGitMock = function () {
  return gracefulGit.call(this, ...Array.from(arguments))
}

const createResolveFromGit = proxyquire('@pnpm/git-resolver', {
  './parsePref': proxyquire('@pnpm/git-resolver/lib/parsePref', {
    'graceful-git': gracefulGitMock,
  }),
  'graceful-git': gracefulGitMock,
}).default

const resolveFromGit = createResolveFromGit({})

test('resolveFromGit() with commit', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedPref: 'github:zkochan/is-negative#163360a8d3ae6bee9524541043197ff356f8ed99',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with no commit', async (t) => {
  // This is repeated twice because there was a bug which caused the normalizedPref
  // to contain the commit hash on second call.
  // The issue occured because .hosted field (which is class from the 'hosted-git-info' package)
  // was mutated. A 'committish' field was added to it.
  for (let i = 0; i < 2; i++) {
    const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative' })
    t.deepEqual(resolveResult, {
      id: 'github.com/zkochan/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c',
      normalizedPref: 'github:zkochan/is-negative',
      resolution: {
        tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
      },
      resolvedVia: 'git-repository',
    })
  }
  t.end()
})

test('resolveFromGit() with branch', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#canary' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/4c39fbc124cd4944ee51cb082ad49320fab58121',
    normalizedPref: 'github:zkochan/is-negative#canary',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/4c39fbc124cd4944ee51cb082ad49320fab58121',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with tag', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#2.0.1' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with tag (v-prefixed tag)', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#v0.0.7' })
  t.deepEqual(resolveResult, {
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with strict semver', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#semver:1.0.0' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/163360a8d3ae6bee9524541043197ff356f8ed99',
    normalizedPref: 'github:zkochan/is-negative#semver:1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with strict semver (v-prefixed tag)', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#semver:v0.0.7' })
  t.deepEqual(resolveResult, {
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#semver:v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with range semver', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'zkochan/is-negative#semver:^1.0.0' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
    normalizedPref: 'github:zkochan/is-negative#semver:^1.0.0',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() with range semver (v-prefixed tag)', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'andreineculau/npm-publish-git#semver:<=v0.0.7' })
  t.deepEqual(resolveResult, {
    id: 'github.com/andreineculau/npm-publish-git/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    normalizedPref: 'github:andreineculau/npm-publish-git#semver:<=v0.0.7',
    resolution: {
      tarball: 'https://codeload.github.com/andreineculau/npm-publish-git/tar.gz/a2f8d94562884e9529cb12c0818312ac87ab7f0b',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() fails when ref not found', async (t) => {
  try {
    await resolveFromGit({ pref: 'zkochan/is-negative#bad-ref' })
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Could not resolve bad-ref to a commit of git://github.com/zkochan/is-negative.git.', 'throws the expected error message')
    t.end()
  }
})

test('resolveFromGit() fails when semver ref not found', async (t) => {
  try {
    await resolveFromGit({ pref: 'zkochan/is-negative#semver:^100.0.0' })
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Could not resolve ^100.0.0 to a commit of git://github.com/zkochan/is-negative.git. Available versions are: 1.0.0, 1.0.1, 2.0.0, 2.0.1, 2.0.2, 2.1.0', 'throws the expected error message')
    t.end()
  }
})

test('resolveFromGit() with commit from non-github repo', async (t) => {
  // TODO: make it pass on Windows
  if (isWindows()) {
    t.end()
    return
  }
  const localPath = process.cwd()
  const resolveResult = await resolveFromGit({ pref: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc` })
  t.deepEqual(resolveResult, {
    id: `${localPath}/988c61e11dc8d9ca0b5580cb15291951812549dc`,
    normalizedPref: `git+file://${localPath}#988c61e11dc8d9ca0b5580cb15291951812549dc`,
    resolution: {
      commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
      repo: `file://${localPath}`,
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// TODO: make it pass on CI servers
test.skip('resolveFromGit() with commit from non-github repo with no commit', async (t) => {
  const localPath = path.resolve('..', '..')
  const result = await git(['rev-parse', 'origin/master'], { retries: 0 })
  const hash: string = result.stdout.trim()
  const resolveResult = await resolveFromGit({ pref: `git+file://${localPath}` })
  t.deepEqual(resolveResult, {
    id: `${localPath}/${hash}`,
    normalizedPref: `git+file://${localPath}`,
    resolution: {
      commit: hash,
      repo: `file://${localPath}`,
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with commit', async (t) => {
  // TODO: make it pass on Windows
  if (isWindows()) {
    t.end()
    return
  }
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  t.deepEqual(resolveResult, {
    id: 'bitbucket.org/pnpmjs/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/988c61e11dc8d9ca0b5580cb15291951812549dc.tar.gz',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with no commit', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  t.deepEqual(resolveResult, {
    id: `bitbucket.org/pnpmjs/git-resolver/${hash}`,
    normalizedPref: 'bitbucket:pnpmjs/git-resolver',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with branch', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#master' })
  const result = await git(['ls-remote', '--refs', 'https://bitbucket.org/pnpmjs/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  t.deepEqual(resolveResult, {
    id: `bitbucket.org/pnpmjs/git-resolver/${hash}`,
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#master',
    resolution: {
      tarball: `https://bitbucket.org/pnpmjs/git-resolver/get/${hash}.tar.gz`,
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// Stopped working. Environmental issue.
test.skip('resolveFromGit() bitbucket with tag', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'bitbucket:pnpmjs/git-resolver#0.3.4' })
  t.deepEqual(resolveResult, {
    id: 'bitbucket.org/pnpmjs/git-resolver/87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    normalizedPref: 'bitbucket:pnpmjs/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() gitlab with commit', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc' })
  t.deepEqual(resolveResult, {
    id: 'gitlab.com/pnpm/git-resolver/988c61e11dc8d9ca0b5580cb15291951812549dc',
    normalizedPref: 'gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
    resolution: {
      tarball: 'https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=988c61e11dc8d9ca0b5580cb15291951812549dc',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() gitlab with no commit', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver' })
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  t.deepEqual(resolveResult, {
    id: `gitlab.com/pnpm/git-resolver/${hash}`,
    normalizedPref: 'gitlab:pnpm/git-resolver',
    resolution: {
      tarball: `https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() gitlab with branch', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#master' })
  const result = await git(['ls-remote', '--refs', 'https://gitlab.com/pnpm/git-resolver.git', 'master'], { retries: 0 })
  const hash: string = result.stdout.trim().split('\t')[0]
  t.deepEqual(resolveResult, {
    id: `gitlab.com/pnpm/git-resolver/${hash}`,
    normalizedPref: 'gitlab:pnpm/git-resolver#master',
    resolution: {
      tarball: `https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=${hash}`,
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() gitlab with tag', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'gitlab:pnpm/git-resolver#0.3.4' })
  t.deepEqual(resolveResult, {
    id: 'gitlab.com/pnpm/git-resolver/87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    normalizedPref: 'gitlab:pnpm/git-resolver#0.3.4',
    resolution: {
      tarball: 'https://gitlab.com/pnpm/git-resolver/repository/archive.tar.gz?ref=87cf6a67064d2ce56e8cd20624769a5512b83ff9',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() normalizes full url', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'git+ssh://git@github.com:zkochan/is-negative.git#2.0.1' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() normalizes full url (alternative form)', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'git+ssh://git@github.com/zkochan/is-negative.git#2.0.1' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolveFromGit() normalizes full url (alternative form 2)', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'https://github.com/zkochan/is-negative.git#2.0.1' })
  t.deepEqual(resolveResult, {
    id: 'github.com/zkochan/is-negative/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:zkochan/is-negative#2.0.1',
    resolution: {
      tarball: 'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

// This test relies on implementation detail.
// current implementation does not try git ls-remote --refs on pref with full commit hash, this fake repo url will pass.
test('resolveFromGit() private repo with commit hash', async (t) => {
  const resolveResult = await resolveFromGit({ pref: 'fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5' })
  t.deepEqual(resolveResult, {
    id: 'github.com/fake/private-repo/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    normalizedPref: 'github:fake/private-repo#2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
    resolution: {
      commit: '2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
      repo: 'git+ssh://git@github.com/fake/private-repo.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})

test('resolve a private repository using the HTTPS protocol and an auth token', async (t) => {
  gracefulGit = async (args: string[]) => {
    if (!args.includes('https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git')) throw new Error('')
    if (args.includes('--refs')) {
      return {
        stdout: '\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/master\
',
      }
    }
    return { stdout: '0000000000000000000000000000000000000000\tHEAD' }
  }
  const resolveResult = await resolveFromGit({ pref: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git' })
  t.deepEqual(resolveResult, {
    id: '0000000000000000000000000000000000000000+x-oauth-basic@github.com/foo/bar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    normalizedPref: 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
    resolution: {
      commit: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      repo: 'https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git',
      type: 'git',
    },
    resolvedVia: 'git-repository',
  })
  t.end()
})
