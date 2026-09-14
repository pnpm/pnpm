import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, it } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'

import { authorNameFromField, bugsUrlFromField, getPkgMetadata, repositoryFromField } from '../lib/getPkgMetadata.js'

const DEFAULT_REGISTRY_OPTS = {
  registriesByScope: {
    default: 'https://registry.npmjs.org/',
    '@jsr': 'https://npm.jsr.io/',
  },
}

function writeCafsFile (storeDir: string, digest: string, content: string): void {
  const filePath = path.join(storeDir, 'files', digest.slice(0, 2), digest.slice(2))
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, content)
}

describe('getPkgMetadata', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(() => {
    storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-sbom-metadata-test-'))
    storeIndex = new StoreIndex(storeDir)
  })

  afterAll(() => {
    storeIndex.close()
    fs.rmSync(storeDir, { recursive: true, force: true })
  })

  const defaultOpts = () => ({
    storeDir,
    storeIndex,
    lockfileDir: '/tmp/project',
    virtualStoreDirMaxLength: 120,
  })

  it('should extract metadata from a registry package', async () => {
    const digest = 'aa11bb22cc33dd44'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'express',
      version: '4.18.2',
      license: 'MIT',
      description: 'Fast web framework',
      author: { name: 'Test Author' },
    }))

    const integrity = 'sha512-sbom/test001'
    const pkgId = 'express@4.18.2'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(storeIndexKey(integrity, pkgId), filesIndex)

    const result = await getPkgMetadata(
      pkgId as DepPath,
      { resolution: { integrity } },
      DEFAULT_REGISTRY_OPTS,
      defaultOpts()
    )

    expect(result.license).toBe('MIT')
    expect(result.description).toBe('Fast web framework')
    expect(result.author).toBe('Test Author')
  })

  it('should extract metadata from a git dependency using the real store key format', async () => {
    const digest = 'dd44ee55ff660011'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'left-pad',
      version: '1.3.0',
      license: 'MIT',
      description: 'String left pad',
      author: 'Steve Mao',
    }))

    // The installer stores git packages under just the git URL, without the
    // package name prefix. getPkgMetadata must strip the prefix from depPath
    // via packageIdFromSnapshot to match.
    const gitUrl = 'git+https://github.com/stevemao/left-pad.git#2fca6157fcca165438e0f9495cf0e5a4e6f71349'
    const depPath = `left-pad@${gitUrl}` as DepPath
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(gitHostedStoreIndexKey(gitUrl, { built: true }), filesIndex)

    const result = await getPkgMetadata(
      depPath,
      {
        resolution: {
          type: 'git',
          repo: 'https://github.com/stevemao/left-pad.git',
          commit: '2fca6157fcca165438e0f9495cf0e5a4e6f71349',
        },
      },
      DEFAULT_REGISTRY_OPTS,
      defaultOpts()
    )

    expect(result.license).toBe('MIT')
    expect(result.description).toBe('String left pad')
    expect(result.author).toBe('Steve Mao')
  })

  it('should return empty metadata when store entry is missing', async () => {
    const depPath = 'missing@git+https://github.com/user/missing.git#deadbeef' as DepPath

    const result = await getPkgMetadata(
      depPath,
      {
        resolution: {
          type: 'git',
          repo: 'https://github.com/user/missing.git',
          commit: 'deadbeef',
        },
      },
      DEFAULT_REGISTRY_OPTS,
      defaultOpts()
    )

    expect(result).toEqual({})
  })
})

describe('bugsUrlFromField', () => {
  it('keeps well-formed http(s) URLs and normalizes the scheme', () => {
    expect(bugsUrlFromField('https://github.com/a/b/issues')).toBe('https://github.com/a/b/issues')
    expect(bugsUrlFromField('http://example.com/bugs')).toBe('http://example.com/bugs')
    // Uppercase scheme is accepted and normalized to lowercase by `new URL`.
    expect(bugsUrlFromField('HTTPS://github.com/a/b/issues')).toBe('https://github.com/a/b/issues')
    expect(bugsUrlFromField('  https://github.com/a/b/issues  ')).toBe('https://github.com/a/b/issues')
    expect(bugsUrlFromField({ url: 'https://github.com/a/b/issues' })).toBe('https://github.com/a/b/issues')
  })

  it('normalizes away control characters and whitespace instead of emitting them raw', () => {
    // `new URL` strips CR/LF/tab and percent-encodes spaces, so a crafted value
    // can't push raw whitespace or control chars into the SBOM url field.
    expect(bugsUrlFromField('https://example.com/\r\nSet-Cookie: x')).toBe('https://example.com/Set-Cookie:%20x')
    expect(bugsUrlFromField('https://example.com/a b')).toBe('https://example.com/a%20b')
  })

  it('strips embedded credentials so the SBOM does not leak them', () => {
    expect(bugsUrlFromField('https://user:token@tracker.example/a/b/issues')).toBe('https://tracker.example/a/b/issues')
    expect(bugsUrlFromField('https://only-user@tracker.example/x')).toBe('https://tracker.example/x')
    expect(bugsUrlFromField({ url: 'https://u:p@tracker.example/i' })).toBe('https://tracker.example/i')
  })

  it('drops malformed URLs, non-http schemes, emails, and missing values', () => {
    expect(bugsUrlFromField('https://')).toBeUndefined()
    expect(bugsUrlFromField('not a url')).toBeUndefined()
    expect(bugsUrlFromField('bugs@example.com')).toBeUndefined()
    expect(bugsUrlFromField('mailto:bugs@example.com')).toBeUndefined()
    expect(bugsUrlFromField({ email: 'bugs@example.com' })).toBeUndefined()
    expect(bugsUrlFromField(undefined)).toBeUndefined()
  })
})

describe('authorNameFromField', () => {
  it('reads the name out of both manifest shapes', () => {
    expect(authorNameFromField('Jane Doe')).toBe('Jane Doe')
    expect(authorNameFromField({ name: 'Jane Doe', email: 'jane@example.com' })).toBe('Jane Doe')
  })

  it('drops a blank name so SPDX never emits the nameless actor "Person: "', () => {
    expect(authorNameFromField('')).toBeUndefined()
    expect(authorNameFromField(' \t\n')).toBeUndefined()
    expect(authorNameFromField({ name: '' })).toBeUndefined()
    expect(authorNameFromField({ name: '   ', email: 'jane@example.com' })).toBeUndefined()
  })

  it('drops values that name nobody', () => {
    expect(authorNameFromField(undefined)).toBeUndefined()
    expect(authorNameFromField({ email: 'jane@example.com' })).toBeUndefined()
    expect(authorNameFromField({ name: 42 })).toBeUndefined()
  })
})

describe('repositoryFromField', () => {
  it('expands the shorthands hosted-git-info knows to the URL npm derives', () => {
    expect(repositoryFromField('vercel/ms')).toBe('git+https://github.com/vercel/ms.git')
    expect(repositoryFromField('acme/widgets.git')).toBe('git+https://github.com/acme/widgets.git')
    expect(repositoryFromField('  vercel/ms  ')).toBe('git+https://github.com/vercel/ms.git')
    expect(repositoryFromField({ type: 'git', url: 'acme/widgets' })).toBe('git+https://github.com/acme/widgets.git')
    expect(repositoryFromField('github:vercel/ms')).toBe('git+https://github.com/vercel/ms.git')
    expect(repositoryFromField('gitlab:acme/widgets')).toBe('git+https://gitlab.com/acme/widgets.git')
    expect(repositoryFromField('bitbucket:acme/widgets')).toBe('git+https://bitbucket.org/acme/widgets.git')
    expect(repositoryFromField('gitlab:foo/bar/baz')).toBe('git+https://gitlab.com/foo/bar/baz.git')
    expect(repositoryFromField('owner/repo#main')).toBe('git+https://github.com/owner/repo.git#main')
    expect(repositoryFromField('git@github.com:foo/bar.git')).toBe('git+https://github.com/foo/bar.git')
  })

  it('keeps absolute URLs of other schemes', () => {
    for (const url of [
      'https://github.com/foo/bar.git',
      'git://github.com/foo/bar.git',
      'git+https://github.com/foo/bar.git',
      'git+ssh://git@github.com/foo/bar.git',
      'ssh://git@github.com/foo/bar.git',
    ]) {
      expect(repositoryFromField(url)).toBe(url)
    }
  })

  it('completes a URL missing a slash', () => {
    expect(repositoryFromField('https:/github.com/foo/bar.git')).toBe('https://github.com/foo/bar.git')
  })

  it('strips the userinfo of an http(s) URL but keeps an ssh login', () => {
    expect(repositoryFromField('https://user:token@github.com/foo/bar')).toBe('https://github.com/foo/bar')
    expect(repositoryFromField('https://token@github.com/foo/bar')).toBe('https://github.com/foo/bar')
    expect(repositoryFromField('git+https://token@github.com/foo/bar.git')).toBe('git+https://github.com/foo/bar.git')
    expect(repositoryFromField('git+ssh://git@github.com/foo/bar.git')).toBe('git+ssh://git@github.com/foo/bar.git')
    expect(repositoryFromField('ssh://git:token@github.com/foo/bar.git')).toBe('ssh://github.com/foo/bar.git')
    expect(repositoryFromField('not-ssh://token@example.com/foo/bar')).toBe('not-ssh://example.com/foo/bar')
    expect(repositoryFromField('https://github.com/foo/bar/baz@qux')).toBe('https://github.com/foo/bar/baz@qux')
  })

  // Expanding the shorthand in one pnpm version alone would split the two.
  it('keeps a gist URL and drops the gist shorthand', () => {
    expect(repositoryFromField('gist:11081aaa281')).toBeUndefined()
    expect(repositoryFromField('https://gist.github.com/11081aaa281')).toBe('https://gist.github.com/11081aaa281')
  })

  it('does not treat userinfo lookalikes in the query as credentials', () => {
    // The query, not the authority, carries the `@`: the URL must come out
    // unchanged, not re-pointed at the query's host.
    expect(
      repositoryFromField('https://github.com?x=user:pass@evil.example/repo')
    ).toBe('https://github.com/?x=user:pass@evil.example/repo')
  })

  it('percent-encodes whitespace in URLs', () => {
    expect(repositoryFromField('https://example.com/a b')).toBe('https://example.com/a%20b')
  })

  it('drops an incomplete percent escape', () => {
    expect(repositoryFromField('https://example.com/%zz')).toBeUndefined()
    expect(repositoryFromField('https://example.com/%')).toBeUndefined()
    // The hosted parser decodes a shorthand's committish, so this `%251`
    // reaches the derived URL as a stray `%1`.
    expect(repositoryFromField('owner/repo#release%251')).toBeUndefined()
  })

  it('drops absolute URLs that do not parse', () => {
    for (const value of ['https://', 'http://user:pass@']) {
      expect(repositoryFromField(value)).toBeUndefined()
    }
  })

  it('drops values that name no repository', () => {
    for (const value of [
      'foo@example.com',
      'a/b/c',
      '/abs/path',
      '.hidden/repo',
      'owner/',
      'owner',
      'owner /repo',
      // Only a project name, with no owner.
      'github:owner',
      'mailto:bugs@example.com',
      'git+file:/tmp/repo',
      '',
      '   ',
    ]) {
      expect(repositoryFromField(value)).toBeUndefined()
    }
    expect(repositoryFromField({ email: 'foo@example.com' })).toBeUndefined()
    expect(repositoryFromField(undefined)).toBeUndefined()
  })
})
