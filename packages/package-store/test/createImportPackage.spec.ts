import fs = require('fs')
import path = require('path')

const fsMock = {} as any // eslint-disable-line
jest.mock('mz/fs', () => {
  const { constants } = jest.requireActual('mz/fs')
  fsMock.constants = constants
  return fsMock
})
jest.mock('path-temp', () => (dir: string) => path.join(dir, '_tmp'))
jest.mock('rename-overwrite', () => jest.fn())

// eslint-disable-next-line
import createImportPackage from '@pnpm/package-store/lib/storeController/createImportPackage'

test('packageImportMethod=auto: clone files by default', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = jest.fn()
  fsMock.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('clone')
  expect(fsMock.copyFile).toBeCalledWith(
    path.join('hash1'),
    path.join('project', '_tmp', 'package.json'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
  expect(fsMock.copyFile).toBeCalledWith(
    path.join('hash2'),
    path.join('project', '_tmp', 'index.js'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
})

test('packageImportMethod=auto: link files if cloning fails', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = jest.fn(() => {
    throw new Error('This file system does not support cloning')
  })
  fsMock.link = jest.fn()
  fsMock.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.link).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(fsMock.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.copyFile).toBeCalled()
  fsMock.copyFile.mockClear()

  // The copy function will not be called again
  expect(await importPackage('project2/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.copyFile).not.toBeCalled()
  expect(fsMock.link).toBeCalledWith(path.join('hash1'), path.join('project2', '_tmp', 'package.json'))
  expect(fsMock.link).toBeCalledWith(path.join('hash2'), path.join('project2', '_tmp', 'index.js'))
})

test('packageImportMethod=auto: link files if cloning fails and even hard linking fails but not with EXDEV error', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = jest.fn(() => {
    throw new Error('This file system does not support cloning')
  })
  let linkFirstCall = true
  fsMock.link = jest.fn(() => {
    if (linkFirstCall) {
      linkFirstCall = false
      throw new Error()
    }
  })
  fsMock.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.link).toBeCalledTimes(2)
  expect(fsMock.copyFile).toBeCalledTimes(1)
})

test('packageImportMethod=auto: chooses copying if cloning and hard linking is not possible', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.copyFile = jest.fn((src: string, dest: string, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw new Error('This file system does not support cloning')
    }
  })
  fsMock.link = jest.fn(() => {
    throw new Error('EXDEV: cross-device link not permitted')
  })
  fsMock.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('copy')
  expect(fsMock.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.copyFile).toBeCalledTimes(2)
})

test('packageImportMethod=hardlink: fall back to copying if hardlinking fails', async () => {
  const importPackage = createImportPackage('hardlink')
  fsMock.link = jest.fn((src: string, dest: string) => {
    if (dest.endsWith('license')) {
      const err = new Error('')
      err['code'] = 'EEXIST'
      throw err
    }
    throw new Error('This file system does not support hard linking')
  })
  fsMock.copyFile = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
      license: 'hash3',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.link).toBeCalledTimes(3)
  expect(fsMock.copyFile).toBeCalledTimes(2) // One time the target already exists, so it won't be copied
  expect(fsMock.copyFile).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(fsMock.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
})
