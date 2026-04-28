import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, test } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, StoreIndex, storeIndexKey } from '@pnpm/store.index'

import { getPkgInfo } from '../lib/getPkgInfo.js'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
  '@jsr': 'https://npm.jsr.io/',
}

function writeCafsFile (storeDir: string, digest: string, content: string): void {
  const dir = path.join(storeDir, 'files', digest.slice(0, 2))
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, digest.slice(2)), content)
}

describe('getPkgInfo', () => {
  let storeDir: string
  let storeIndex: StoreIndex

  beforeAll(() => {
    storeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-license-test-'))
    storeIndex = new StoreIndex(storeDir)
  })

  afterAll(() => {
    storeIndex.close()
    fs.rmSync(storeDir, { recursive: true, force: true })
  })

  const defaultGetOpts = () => ({
    storeDir,
    storeIndex,
    virtualStoreDir: 'virtual-store-dir',
    modulesDir: 'modules-dir',
    dir: 'workspace-dir',
    virtualStoreDirMaxLength: 120,
  })

  test('should throw when registry package is not in the store', async () => {
    await expect(
      getPkgInfo(
        {
          name: 'bogus-package',
          version: '1.0.0',
          id: 'bogus-package@1.0.0',
          depPath: 'bogus-package@1.0.0',
          snapshot: {
            resolution: {
              integrity: 'integrity-sha',
            },
          },
          registries: DEFAULT_REGISTRIES,
        },
        defaultGetOpts()
      )
    ).rejects.toThrow(/Failed to find package index file for bogus-package@1\.0\.0 \(at .*\), please consider running 'pnpm install'/)
  })

  test('should throw when git dependency is not in the store', async () => {
    const depPath = 'left-pad@git+https://github.com/stevemao/left-pad.git#2fca6157'
    await expect(
      getPkgInfo(
        {
          name: 'left-pad',
          version: '1.3.0',
          id: depPath,
          depPath,
          snapshot: {
            resolution: {
              type: 'git',
              repo: 'https://github.com/stevemao/left-pad.git',
              commit: '2fca6157',
            },
          },
          registries: DEFAULT_REGISTRIES,
        },
        defaultGetOpts()
      )
    ).rejects.toThrow(/Failed to find package index file for/)
  })

  test('should extract license from a registry package in the store', async () => {
    const digest = 'ee00ff1122334455'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'express',
      version: '4.18.2',
      license: 'MIT',
      description: 'Fast web framework',
      author: { name: 'Test Author' },
      homepage: 'https://expressjs.com/',
      repository: { url: 'https://github.com/expressjs/express' },
    }))

    const pkgId = 'express@4.18.2'
    const integrity = 'sha512-test/integrity001'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(storeIndexKey(integrity, pkgId), filesIndex)

    const result = await getPkgInfo(
      {
        name: 'express',
        version: '4.18.2',
        id: pkgId,
        depPath: pkgId,
        snapshot: {
          resolution: { integrity },
        },
        registries: DEFAULT_REGISTRIES,
      },
      defaultGetOpts()
    )

    expect(result.license).toBe('MIT')
    expect(result.author).toBe('Test Author')
    expect(result.description).toBe('Fast web framework')
  })

  test('should extract license from a git dependency in the store', async () => {
    const digest = 'ff99aa8877665544'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'left-pad',
      version: '1.3.0',
      license: 'MIT',
      description: 'String left pad',
      author: 'Steve Mao',
      repository: { url: 'https://github.com/stevemao/left-pad' },
    }))

    // The installer stores git packages under just the git URL, without the
    // package name prefix. packageIdFromSnapshot strips the prefix when the
    // caller (lockfileToLicenseNodeTree) builds the id for getPkgInfo.
    const gitUrl = 'git+https://github.com/stevemao/left-pad.git#2fca6157fcca165438e0f9495cf0e5a4e6f71349'
    const depPath = `left-pad@${gitUrl}`
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(gitHostedStoreIndexKey(gitUrl, { built: true }), filesIndex)

    const result = await getPkgInfo(
      {
        name: 'left-pad',
        version: '1.3.0',
        id: gitUrl,
        depPath,
        snapshot: {
          resolution: {
            type: 'git',
            repo: 'https://github.com/stevemao/left-pad.git',
            commit: '2fca6157fcca165438e0f9495cf0e5a4e6f71349',
          },
        },
        registries: DEFAULT_REGISTRIES,
      },
      defaultGetOpts()
    )

    expect(result.license).toBe('MIT')
    expect(result.author).toBe('Steve Mao')
    expect(result.description).toBe('String left pad')
  })
})
