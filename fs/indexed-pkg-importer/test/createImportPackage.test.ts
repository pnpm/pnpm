import fs, { type BigIntStats } from 'fs'
import path from 'path'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import gfs from '@pnpm/graceful-fs'
import { globalInfo } from '@pnpm/logger'
import { jest } from '@jest/globals'

const testOnLinuxOnly = (process.platform === 'darwin' || process.platform === 'win32') ? test.skip : test

jest.mock('@pnpm/graceful-fs', () => {
  const { access, promises } = jest.requireActual<typeof fs>('fs')
  const fsMock = {
    mkdirSync: promises.mkdir,
    readdirSync: promises.readdir,
    access,
    copyFileSync: jest.fn(),
    linkSync: jest.fn(),
    statSync: jest.fn(),
  }
  return {
    __esModule: true,
    default: fsMock,
  }
})
jest.mock('path-temp', () => ({ fastPathTemp: (file: string) => `${file}_tmp` }))
jest.mock('rename-overwrite', () => ({ sync: jest.fn() }))
jest.mock('fs-extra', () => ({
  copySync: jest.fn(),
}))
jest.mock('@pnpm/logger', () => ({
  logger: jest.fn(() => ({ debug: jest.fn() })),
  globalWarn: jest.fn(),
  globalInfo: jest.fn(),
}))

beforeEach(() => {
  jest.mocked(gfs.copyFileSync).mockClear()
  jest.mocked(gfs.linkSync).mockClear()
  jest.mocked(globalInfo).mockReset()
})

testOnLinuxOnly('packageImportMethod=auto: clone files by default', () => {
  const importPackage = createIndexedPkgImporter('auto')
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('clone')
  expect(gfs.copyFileSync).toHaveBeenCalledWith(
    path.join('hash1'),
    path.join('project', 'package_tmp', 'package.json'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
  expect(gfs.copyFileSync).toHaveBeenCalledWith(
    path.join('hash2'),
    path.join('project', 'package_tmp', 'index.js'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
})

testOnLinuxOnly('packageImportMethod=auto: link files if cloning fails', () => {
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw new Error('This file system does not support cloning')
  })
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project', 'package_tmp', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package_tmp', 'index.js'))
  expect(gfs.copyFileSync).toHaveBeenCalled()
  jest.mocked(gfs.copyFileSync).mockClear()

  // The copy function will not be called again
  expect(importPackage('project2/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.copyFileSync).not.toHaveBeenCalled()
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project2', 'package_tmp', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project2', 'package_tmp', 'index.js'))
})

testOnLinuxOnly('packageImportMethod=auto: link files if cloning fails and even hard linking fails but not with EXDEV error', () => {
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw new Error('This file system does not support cloning')
  })
  let linkFirstCall = true
  jest.mocked(gfs.linkSync).mockImplementation(() => {
    if (linkFirstCall) {
      linkFirstCall = false
      throw new Error()
    }
  })
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package_tmp', 'index.js'))
  expect(gfs.linkSync).toHaveBeenCalledTimes(2)
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(1)
})

testOnLinuxOnly('packageImportMethod=auto: chooses copying if cloning and hard linking is not possible', () => {
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation((src, dest, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw new Error('This file system does not support cloning')
    }
  })
  jest.mocked(gfs.linkSync).mockImplementation(() => {
    throw new Error('EXDEV: cross-device link not permitted')
  })
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('copy')
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package_tmp', 'index.js'))
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(2)
})

testOnLinuxOnly('packageImportMethod=hardlink: fall back to copying if hardlinking fails', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.linkSync).mockImplementation((src, dest) => {
    if (dest.toString().endsWith('license')) {
      throw Object.assign(new Error(''), { code: 'EEXIST' })
    }
    throw new Error('This file system does not support hard linking')
  })
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
      license: 'hash3',
    },
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledTimes(3)
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(2) // One time the target already exists, so it won't be copied
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project', 'package_tmp', 'package.json'))
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package_tmp', 'index.js'))
})

test('packageImportMethod=hardlink does not relink package from store if package.json is linked from the store', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.statSync).mockReturnValue({ ino: BigInt(1) } as fs.BigIntStats)
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'store',
  })).toBe(undefined)
})

test('packageImportMethod=hardlink relinks package from store if package.json is not linked from the store', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  let ino = 0
  jest.mocked(gfs.statSync as jest.Mock).mockImplementation(() => ({ ino: ++ino }))
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'store',
  })).toBe('hardlink')
  expect(globalInfo).toHaveBeenCalledWith('Relinking project/package from the store')
})

test('packageImportMethod=hardlink does not relink package from store if package.json is not present in the store', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.statSync).mockImplementation(((file: string) => {
    expect(typeof file).toBe('string')
    return { ino: BigInt(1) } as BigIntStats
  }) as unknown as typeof gfs.statSync)
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    resolvedFrom: 'store',
  })).toBe(undefined)
})

test('packageImportMethod=hardlink links packages when they are not found', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.statSync).mockImplementation(((file: string) => {
    if (file === path.join('project/package', 'package.json')) {
      throw Object.assign(new Error(), { code: 'ENOENT' })
    }
    return { ino: BigInt(0) } as BigIntStats
  }) as unknown as typeof gfs.statSync)
  expect(importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    resolvedFrom: 'store',
  })).toBe('hardlink')
  expect(globalInfo).not.toHaveBeenCalledWith('Relinking project/package from the store')
})
