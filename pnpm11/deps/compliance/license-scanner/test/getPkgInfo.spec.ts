import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, beforeAll, describe, expect, test } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { gitHostedStoreIndexKey, StoreIndex, storeIndexKey } from '@pnpm/store.index'

import { getPkgInfo } from '../lib/getPkgInfo.js'

export const DEFAULT_REGISTRIES_BY_SCOPE = {
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
          registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
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
          registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
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
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
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
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      defaultGetOpts()
    )

    expect(result.license).toBe('MIT')
    expect(result.author).toBe('Steve Mao')
    expect(result.description).toBe('String left pad')
  })

  test.each(['node_modules', 'custom_modules'])('uses node_modules inside a custom virtual store with modulesDir %s', async (modulesDir) => {
    const digest = 'dd00ff1122334455'
    writeCafsFile(storeDir, digest, JSON.stringify({ name: 'express', version: '4.18.2', license: 'MIT' }))
    const id = 'express@4.18.2'
    const integrity = 'sha512-custom/modules'
    storeIndex.set(storeIndexKey(integrity, id), {
      algo: 'sha256',
      files: new Map([['package.json', { digest, mode: 0o644, size: 0 }]]),
    })
    const result = await getPkgInfo({
      id,
      depPath: id,
      snapshot: { resolution: { integrity } },
      registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
    }, {
      ...defaultGetOpts(),
      dir: storeDir,
      modulesDir,
      virtualStoreDir: 'virtual-store',
    })
    expect(result.path).toBe(path.join(storeDir, 'virtual-store', id, 'node_modules', 'express'))
  })

  test('should resolve path from hoistedLocations when nodeLinker is hoisted', async () => {
    const digest = '1122334455667788'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'is-positive',
      version: '3.1.0',
      license: 'MIT',
    }))

    const pkgId = 'is-positive@3.1.0'
    const integrity = 'sha512-test/integrity002'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(storeIndexKey(integrity, pkgId), filesIndex)

    const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-hoisted-test-'))
    const hoistedPkgPath = path.join(workspaceDir, 'node_modules', 'is-positive')
    fs.mkdirSync(hoistedPkgPath, { recursive: true })

    const result = await getPkgInfo(
      {
        name: 'is-positive',
        version: '3.1.0',
        id: pkgId,
        depPath: pkgId,
        snapshot: {
          resolution: { integrity },
        },
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      {
        ...defaultGetOpts(),
        dir: workspaceDir,
        lockfileDir: workspaceDir,
        modulesDir: 'node_modules',
        nodeLinker: 'hoisted',
        hoistedLocations: {
          'is-positive@3.1.0': ['node_modules/is-positive'],
        },
      }
    )

    expect(result.path).toBe(hoistedPkgPath)
    expect(result.path!.includes('.pnpm')).toBeFalsy()

    fs.rmSync(workspaceDir, { recursive: true, force: true })
  })

  test('should reject unsafe hoisted locations and fallback safely', async () => {
    const digest = '3344556677889900'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'is-positive',
      version: '3.1.0',
      license: 'MIT',
    }))

    const pkgId = 'is-positive@3.1.0'
    const integrity = 'sha512-test/integrity004'
    storeIndex.set(storeIndexKey(integrity, pkgId), {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    })

    const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-hoisted-unsafe-test-'))
    const safePkgPath = path.join(workspaceDir, 'node_modules', 'is-positive')
    fs.mkdirSync(safePkgPath, { recursive: true })
    fs.writeFileSync(path.join(safePkgPath, 'package.json'), JSON.stringify({ name: 'is-positive', version: '3.1.0' }))

    const result = await getPkgInfo(
      {
        name: 'is-positive',
        version: '3.1.0',
        id: pkgId,
        depPath: pkgId,
        snapshot: { resolution: { integrity } },
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      {
        ...defaultGetOpts(),
        dir: workspaceDir,
        lockfileDir: workspaceDir,
        modulesDir: 'node_modules',
        nodeLinker: 'hoisted',
        hoistedLocations: {
          'is-positive@3.1.0': ['../../outside', '..\\..\\outside', '/etc/passwd'],
        },
      }
    )

    expect(result.path).toBe(safePkgPath)

    fs.rmSync(workspaceDir, { recursive: true, force: true })
  })

  test('should resolve path to node_modules when shamefullyHoist is enabled', async () => {
    const digest = '2233445566778899'
    writeCafsFile(storeDir, digest, JSON.stringify({
      name: 'is-positive',
      version: '3.1.0',
      license: 'MIT',
    }))

    const pkgId = 'is-positive@3.1.0'
    const integrity = 'sha512-test/integrity002'
    const filesIndex: PackageFilesIndex = {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest, mode: 0o644, size: 0 }],
      ]),
    }
    storeIndex.set(storeIndexKey(integrity, pkgId), filesIndex)

    const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-shameful-test-'))
    const vsPkgPath = path.join(workspaceDir, 'node_modules', '.pnpm', 'is-positive@3.1.0', 'node_modules', 'is-positive')
    fs.mkdirSync(vsPkgPath, { recursive: true })
    const hoistedPkgPath = path.join(workspaceDir, 'node_modules', 'is-positive')
    fs.symlinkSync(vsPkgPath, hoistedPkgPath, 'junction')

    const result = await getPkgInfo(
      {
        name: 'is-positive',
        version: '3.1.0',
        id: pkgId,
        depPath: pkgId,
        snapshot: {
          resolution: { integrity },
        },
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      {
        ...defaultGetOpts(),
        dir: workspaceDir,
        lockfileDir: workspaceDir,
        modulesDir: 'node_modules',
        virtualStoreDir: path.join(workspaceDir, 'node_modules', '.pnpm'),
        shamefullyHoist: true,
      }
    )

    expect(result.path).toBe(hoistedPkgPath)
    expect(result.path!.includes('.pnpm')).toBeFalsy()

    fs.rmSync(workspaceDir, { recursive: true, force: true })
  })

  test('should resolve distinct paths for two versions of one package when shamefullyHoist is enabled', async () => {
    const digest1 = '1111111111111111'
    writeCafsFile(storeDir, digest1, JSON.stringify({
      name: 'is-positive',
      version: '1.0.0',
      license: 'MIT',
    }))

    const digest2 = '2222222222222222'
    writeCafsFile(storeDir, digest2, JSON.stringify({
      name: 'is-positive',
      version: '3.1.0',
      license: 'MIT',
    }))

    const pkgId1 = 'is-positive@1.0.0'
    const integrity1 = 'sha512-test/integrity001'
    storeIndex.set(storeIndexKey(integrity1, pkgId1), {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest: digest1, mode: 0o644, size: 0 }],
      ]),
    })

    const pkgId2 = 'is-positive@3.1.0'
    const integrity2 = 'sha512-test/integrity002'
    storeIndex.set(storeIndexKey(integrity2, pkgId2), {
      algo: 'sha256',
      files: new Map([
        ['package.json', { digest: digest2, mode: 0o644, size: 0 }],
      ]),
    })

    const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-shameful-versions-test-'))
    const vsPath1 = path.join(workspaceDir, 'node_modules', '.pnpm', 'is-positive@1.0.0', 'node_modules', 'is-positive')
    const vsPath2 = path.join(workspaceDir, 'node_modules', '.pnpm', 'is-positive@3.1.0', 'node_modules', 'is-positive')
    fs.mkdirSync(vsPath1, { recursive: true })
    fs.mkdirSync(vsPath2, { recursive: true })

    const hoistedPkgPath = path.join(workspaceDir, 'node_modules', 'is-positive')
    fs.symlinkSync(vsPath2, hoistedPkgPath, 'junction')

    const result1 = await getPkgInfo(
      {
        name: 'is-positive',
        version: '1.0.0',
        id: pkgId1,
        depPath: pkgId1,
        snapshot: { resolution: { integrity: integrity1 } },
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      {
        ...defaultGetOpts(),
        dir: workspaceDir,
        lockfileDir: workspaceDir,
        modulesDir: 'node_modules',
        virtualStoreDir: path.join(workspaceDir, 'node_modules', '.pnpm'),
        shamefullyHoist: true,
      }
    )

    const result2 = await getPkgInfo(
      {
        name: 'is-positive',
        version: '3.1.0',
        id: pkgId2,
        depPath: pkgId2,
        snapshot: { resolution: { integrity: integrity2 } },
        registriesByScope: DEFAULT_REGISTRIES_BY_SCOPE,
      },
      {
        ...defaultGetOpts(),
        dir: workspaceDir,
        lockfileDir: workspaceDir,
        modulesDir: 'node_modules',
        virtualStoreDir: path.join(workspaceDir, 'node_modules', '.pnpm'),
        shamefullyHoist: true,
      }
    )

    expect(result2.path).toBe(hoistedPkgPath)
    expect(result2.path!.includes('.pnpm')).toBeFalsy()

    expect(result1.path).toBe(vsPath1)
    expect(result1.path!.includes('.pnpm')).toBeTruthy()

    expect(result1.path).not.toBe(result2.path)

    fs.rmSync(workspaceDir, { recursive: true, force: true })
  })
})
