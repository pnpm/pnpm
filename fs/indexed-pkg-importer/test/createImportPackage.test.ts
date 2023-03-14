import fs from 'fs'
import path from 'path'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import gfs from '@pnpm/graceful-fs'
import { globalInfo } from '@pnpm/logger'

jest.mock('@pnpm/graceful-fs', () => {
  const { access, promises } = jest.requireActual('fs')
  const fsMock = {
    mkdir: promises.mkdir,
    readdir: promises.readdir,
    access,
    copyFile: jest.fn(),
    link: jest.fn(),
    stat: jest.fn(),
  }
  return {
    __esModule: true,
    default: fsMock,
  }
})
jest.mock('path-temp', () => (dir: string) => path.join(dir, '_tmp'))
jest.mock('rename-overwrite', () => jest.fn())
jest.mock('fs-extra', () => ({
  copy: jest.fn(),
}))
jest.mock('@pnpm/logger', () => ({
  logger: jest.fn(() => ({ debug: jest.fn() })),
  globalWarn: jest.fn(),
  globalInfo: jest.fn(),
}))

beforeEach(() => {
  ;(gfs.copyFile as jest.Mock).mockClear()
  ;(gfs.link as jest.Mock).mockClear()
  ;(globalInfo as jest.Mock).mockReset()
})

test('packageImportMethod=auto: clone files by default', async () => {
  const importPackage = createIndexedPkgImporter('auto')
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('clone')
  expect(gfs.copyFile).toBeCalledWith(
    path.join('hash1'),
    path.join('project', '_tmp', 'package.json'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
  expect(gfs.copyFile).toBeCalledWith(
    path.join('hash2'),
    path.join('project', '_tmp', 'index.js'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
})

test('packageImportMethod=auto: link files if cloning fails', async () => {
  const importPackage = createIndexedPkgImporter('auto')
  ;(gfs.copyFile as jest.Mock).mockImplementation(async () => {
    throw new Error('This file system does not support cloning')
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(gfs.link).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(gfs.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(gfs.copyFile).toBeCalled()
  ;(gfs.copyFile as jest.Mock).mockClear()

  // The copy function will not be called again
  expect(await importPackage('project2/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(gfs.copyFile).not.toBeCalled()
  expect(gfs.link).toBeCalledWith(path.join('hash1'), path.join('project2', '_tmp', 'package.json'))
  expect(gfs.link).toBeCalledWith(path.join('hash2'), path.join('project2', '_tmp', 'index.js'))
})

test('packageImportMethod=auto: link files if cloning fails and even hard linking fails but not with EXDEV error', async () => {
  const importPackage = createIndexedPkgImporter('auto')
  ;(gfs.copyFile as jest.Mock).mockImplementation(async () => {
    throw new Error('This file system does not support cloning')
  })
  let linkFirstCall = true
  ;(gfs.link as jest.Mock).mockImplementation(async () => {
    if (linkFirstCall) {
      linkFirstCall = false
      throw new Error()
    }
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(gfs.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(gfs.link).toBeCalledTimes(2)
  expect(gfs.copyFile).toBeCalledTimes(1)
})

test('packageImportMethod=auto: chooses copying if cloning and hard linking is not possible', async () => {
  const importPackage = createIndexedPkgImporter('auto')
  ;(gfs.copyFile as jest.Mock).mockImplementation(async (src: string, dest: string, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw new Error('This file system does not support cloning')
    }
  })
  ;(gfs.link as jest.Mock).mockImplementation(() => {
    throw new Error('EXDEV: cross-device link not permitted')
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('copy')
  expect(gfs.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(gfs.copyFile).toBeCalledTimes(2)
})

test('packageImportMethod=hardlink: fall back to copying if hardlinking fails', async () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  ;(gfs.link as jest.Mock).mockImplementation(async (src: string, dest: string) => {
    if (dest.endsWith('license')) {
      throw Object.assign(new Error(''), { code: 'EEXIST' })
    }
    throw new Error('This file system does not support hard linking')
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
      license: 'hash3',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(gfs.link).toBeCalledTimes(3)
  expect(gfs.copyFile).toBeCalledTimes(2) // One time the target already exists, so it won't be copied
  expect(gfs.copyFile).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(gfs.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
})

test('packageImportMethod=hardlink does not relink package from store if package.json is linked from the store', async () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  ;(gfs.stat as jest.Mock).mockReturnValue(Promise.resolve({ ino: 1 }))
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: true,
  })).toBe(undefined)
})

test('packageImportMethod=hardlink relinks package from store if package.json is not linked from the store', async () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  let ino = 0
  ;(gfs.stat as jest.Mock).mockImplementation(async () => ({ ino: ++ino }))
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: true,
  })).toBe('hardlink')
  expect(globalInfo).toBeCalledWith('Relinking project/package from the store')
})

test('packageImportMethod=hardlink does not relink package from store if package.json is not present in the store', async () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  ;(gfs.stat as jest.Mock).mockImplementation(async (file) => {
    expect(typeof file).toBe('string')
    return { ino: 1 }
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: true,
  })).toBe(undefined)
})

test('packageImportMethod=hardlink links packages when they are not found', async () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  ;(gfs.stat as jest.Mock).mockImplementation(async (file) => {
    if (file === path.join('project/package', 'package.json')) {
      throw Object.assign(new Error(), { code: 'ENOENT' })
    }
    return { ino: 0 }
  })
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: true,
  })).toBe('hardlink')
  expect(globalInfo).not.toBeCalledWith('Relinking project/package from the store')
})
