import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, StoreIndex, storeIndexKey } from '@pnpm/store.index'
import type { DepPath } from '@pnpm/types'

import { getPkgMetadata } from '../lib/getPkgMetadata.js'

const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
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
      DEFAULT_REGISTRIES,
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
      DEFAULT_REGISTRIES,
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
      DEFAULT_REGISTRIES,
      defaultOpts()
    )

    expect(result).toEqual({})
  })
})
