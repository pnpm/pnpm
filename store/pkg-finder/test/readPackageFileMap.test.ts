import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import type { GitResolution, Resolution, TarballResolution } from '@pnpm/resolving.resolver-base'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { readPackageFileMap } from '@pnpm/store.pkg-finder'

function createFilesIndex (): PackageFilesIndex {
  return {
    algo: 'sha256',
    files: new Map([
      ['package.json', { digest: 'abc123', mode: 0o644, size: 0 }],
      ['index.js', { digest: 'def456', mode: 0o644, size: 0 }],
    ]),
  }
}

function writeCafsFile (storeDir: string, digest: string, content: string): void {
  const dir = path.join(storeDir, 'files', digest.slice(0, 2))
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, digest.slice(2)), content)
}

describe('readPackageFileMap', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(() => {
    storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pkg-finder-test-'))
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

  it('should resolve registry packages by integrity hash', async () => {
    const integrity = 'sha512-abc123registry'
    const pkgId = 'express@4.18.2'
    const key = storeIndexKey(integrity, pkgId)

    storeIndex.set(key, createFilesIndex())

    const resolution: TarballResolution = {
      integrity,
      tarball: 'https://registry.npmjs.org/express/-/express-4.18.2.tgz',
    }

    const result = await readPackageFileMap(resolution, pkgId, defaultOpts())

    expect(result).toBeDefined()
    expect(result!.has('package.json')).toBe(true)
    expect(result!.has('index.js')).toBe(true)
  })

  it('should resolve git-hosted tarball packages (no type, has tarball)', async () => {
    const pkgId = 'left-pad@https://codeload.github.com/stevemao/left-pad/tar.gz/abc123'
    const key = gitHostedStoreIndexKey(pkgId, { built: true })

    storeIndex.set(key, createFilesIndex())

    const resolution = {
      tarball: 'https://codeload.github.com/stevemao/left-pad/tar.gz/abc123',
    } as TarballResolution

    const result = await readPackageFileMap(resolution, pkgId, defaultOpts())

    expect(result).toBeDefined()
    expect(result!.has('package.json')).toBe(true)
    expect(result!.has('index.js')).toBe(true)
  })

  it('should resolve git dependencies with type "git" and return readable file paths', async () => {
    const digest = 'aabbccdd001122'
    const manifestContent = JSON.stringify({
      name: 'left-pad',
      version: '1.3.0',
      license: 'MIT',
    })
    writeCafsFile(storeDir, digest, manifestContent)

    const pkgId = 'left-pad@git+https://github.com/stevemao/left-pad.git#2fca6157fcca165438e0f9495cf0e5a4e6f71349'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(gitHostedStoreIndexKey(pkgId, { built: true }), filesIndex)

    const resolution: GitResolution = {
      type: 'git',
      repo: 'https://github.com/stevemao/left-pad.git',
      commit: '2fca6157fcca165438e0f9495cf0e5a4e6f71349',
    }

    const result = await readPackageFileMap(resolution, pkgId, defaultOpts())

    expect(result).toBeDefined()
    const manifestPath = result!.get('package.json')!
    expect(fs.existsSync(manifestPath)).toBe(true)

    const parsed = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))
    expect(parsed.name).toBe('left-pad')
    expect(parsed.license).toBe('MIT')
  })

  it('should throw ENOENT when store index has no entry for a git dependency', async () => {
    const pkgId = 'missing-pkg@git+https://github.com/user/missing-pkg.git#deadbeef'

    const resolution: GitResolution = {
      type: 'git',
      repo: 'https://github.com/user/missing-pkg.git',
      commit: 'deadbeef',
    }

    await expect(
      readPackageFileMap(resolution, pkgId, defaultOpts())
    ).rejects.toThrow(/package index not found/)
  })

  it('should return undefined for unknown resolution types', async () => {
    const resolution = { type: 'unknown-type' } as unknown as Resolution

    const result = await readPackageFileMap(resolution, 'some-pkg@1.0.0', defaultOpts())

    expect(result).toBeUndefined()
  })
})
