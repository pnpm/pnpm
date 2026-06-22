import { expect, test } from '@jest/globals'
import { classifyResolution, isGitHostedTarballUrl, type Resolution } from '@pnpm/resolving.resolver-base'

const GIT_COMMIT = '0123456789abcdef0123456789abcdef01234567'

test('classifyResolution() classifies tarball-shaped resolutions', () => {
  expect(classifyResolution({ tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' } as Resolution)).toBe('remoteTarball')
  expect(classifyResolution({ tarball: 'file:foo-1.0.0.tgz' } as Resolution)).toBe('localTarball')
  expect(classifyResolution({ tarball: `https://codeload.github.com/foo/bar/tar.gz/${GIT_COMMIT}` } as Resolution)).toBe('gitHostedTarball')
  // A recognized git-host URL is git-hosted with or without the `gitHosted` flag.
  expect(classifyResolution({ tarball: `https://codeload.github.com/foo/bar/tar.gz/${GIT_COMMIT}`, gitHosted: true } as Resolution)).toBe('gitHostedTarball')
  // A forged `gitHosted: true` on a non-git-host URL is NOT trusted (lockfiles are untrusted
  // input): it stays a `remoteTarball` so the missing-integrity gate still applies.
  expect(classifyResolution({ tarball: 'https://example.com/foo.tgz', gitHosted: true } as Resolution)).toBe('remoteTarball')
})

test('classifyResolution() treats a canonical entry with no tarball URL as a remote tarball', () => {
  // A canonical registry entry omits the URL (reconstructed from name+version), so an empty
  // resolution must still classify as a remote tarball rather than something exempt.
  expect(classifyResolution({} as Resolution)).toBe('remoteTarball')
  expect(classifyResolution({ integrity: 'sha512-x' } as Resolution)).toBe('remoteTarball')
})

test('classifyResolution() classifies typed resolutions', () => {
  expect(classifyResolution({ type: 'directory', directory: '/foo' } as Resolution)).toBe('directory')
  expect(classifyResolution({ type: 'git', repo: 'r', commit: 'c' } as Resolution)).toBe('git')
  expect(classifyResolution({ type: 'binary', url: 'u', integrity: 'i', archive: 'tarball', bin: 'b' } as Resolution)).toBe('binary')
  expect(classifyResolution({ type: 'variations', variants: [] } as Resolution)).toBe('custom')
  expect(classifyResolution({ type: 'custom:cdn' } as Resolution)).toBe('custom')
})

test('classifyResolution() treats a YAML `type: null` as a tarball, not custom', () => {
  expect(classifyResolution({ type: null, tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' } as unknown as Resolution)).toBe('remoteTarball')
})

test('classifyResolution() does not crash on a non-string tarball from a tampered lockfile', () => {
  expect(classifyResolution({ tarball: ['https://attacker.example/foo.tgz'] } as unknown as Resolution)).toBe('remoteTarball')
  expect(classifyResolution({ tarball: 42 } as unknown as Resolution)).toBe('remoteTarball')
})

test('isGitHostedTarballUrl() recognizes git provider archive URLs (case-insensitive)', () => {
  expect(isGitHostedTarballUrl(`https://codeload.github.com/foo/bar/tar.gz/${GIT_COMMIT}`)).toBe(true)
  expect(isGitHostedTarballUrl(`https://gitlab.com/foo/bar/-/archive/${GIT_COMMIT}/bar-${GIT_COMMIT}.tar.gz`)).toBe(true)
  expect(isGitHostedTarballUrl(`https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz?ref=${GIT_COMMIT}`)).toBe(true)
  expect(isGitHostedTarballUrl(`https://bitbucket.org/foo/bar/get/${GIT_COMMIT}.tar.gz`)).toBe(true)
  // A tampered upper-cased host must not slip past as a registry-trusted tarball.
  expect(isGitHostedTarballUrl(`https://CODELOAD.GITHUB.COM/foo/bar/tar.gz/${GIT_COMMIT}`)).toBe(true)
})

test('isGitHostedTarballUrl() rejects non-git-host and non-string inputs', () => {
  expect(isGitHostedTarballUrl('https://registry.npmjs.org/foo/-/foo-1.0.0.tgz')).toBe(false)
  expect(isGitHostedTarballUrl('https://github.com/foo/bar')).toBe(false)
  expect(isGitHostedTarballUrl('https://gitlab.com/foo/bar?download=tar.gz')).toBe(false)
  expect(isGitHostedTarballUrl('https://codeload.github.com/foo/bar/tar.gz/main')).toBe(false)
  expect(isGitHostedTarballUrl('https://gitlab.com/foo/bar/-/archive/main/bar-main.tar.gz')).toBe(false)
  expect(isGitHostedTarballUrl('https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz')).toBe(false)
  expect(isGitHostedTarballUrl('https://bitbucket.org/foo/bar/get/main.tar.gz')).toBe(false)
  expect(isGitHostedTarballUrl(undefined as unknown as string)).toBe(false)
  expect(isGitHostedTarballUrl(['https://codeload.github.com/x/tar.gz'] as unknown as string)).toBe(false)
})
