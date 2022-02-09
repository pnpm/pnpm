import path from 'path'

const fsMock = { promises: {} as any } as any // eslint-disable-line
jest.mock('fs', () => {
  const { access, constants, promises } = jest.requireActual('fs')
  fsMock.constants = constants
  fsMock.promises.mkdir = promises.mkdir
  fsMock.promises.readdir = promises.readdir
  fsMock.access = access
  return fsMock
})
jest.mock('path-temp', () => (dir: string) => path.join(dir, '_tmp'))
jest.mock('rename-overwrite', () => jest.fn())
const globalInfo = jest.fn()
const globalWarn = jest.fn()
const baseLogger = jest.fn(() => ({ debug: jest.fn() }))
baseLogger['globalInfo'] = globalInfo
baseLogger['globalWarn'] = globalWarn
jest.mock('@pnpm/logger', () => baseLogger)

// eslint-disable-next-line
import createImportPackage from '@pnpm/package-store/lib/storeController/createImportPackage'

test('packageImportMethod=auto: clone files by default', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.promises.copyFile = jest.fn()
  fsMock.promises.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('clone')
  expect(fsMock.promises.copyFile).toBeCalledWith(
    path.join('hash1'),
    path.join('project', '_tmp', 'package.json'),
    fsMock.constants.COPYFILE_FICLONE_FORCE
  )
  expect(fsMock.promises.copyFile).toBeCalledWith(
    path.join('hash2'),
    path.join('project', '_tmp', 'index.js'),
    fsMock.constants.COPYFILE_FICLONE_FORCE
  )
})

test('packageImportMethod=auto: link files if cloning fails', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.promises.copyFile = jest.fn(() => {
    throw new Error('This file system does not support cloning')
  })
  fsMock.promises.link = jest.fn()
  fsMock.promises.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.promises.link).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(fsMock.promises.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.promises.copyFile).toBeCalled()
  fsMock.promises.copyFile.mockClear()

  // The copy function will not be called again
  expect(await importPackage('project2/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.promises.copyFile).not.toBeCalled()
  expect(fsMock.promises.link).toBeCalledWith(path.join('hash1'), path.join('project2', '_tmp', 'package.json'))
  expect(fsMock.promises.link).toBeCalledWith(path.join('hash2'), path.join('project2', '_tmp', 'index.js'))
})

test('packageImportMethod=auto: link files if cloning fails and even hard linking fails but not with EXDEV error', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.promises.copyFile = jest.fn(() => {
    throw new Error('This file system does not support cloning')
  })
  let linkFirstCall = true
  fsMock.promises.link = jest.fn(() => {
    if (linkFirstCall) {
      linkFirstCall = false
      throw new Error()
    }
  })
  fsMock.promises.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.promises.link).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.promises.link).toBeCalledTimes(2)
  expect(fsMock.promises.copyFile).toBeCalledTimes(1)
})

test('packageImportMethod=auto: chooses copying if cloning and hard linking is not possible', async () => {
  const importPackage = createImportPackage('auto')
  fsMock.promises.copyFile = jest.fn((src: string, dest: string, flags?: number) => {
    if (flags === fsMock.constants.COPYFILE_FICLONE_FORCE) {
      throw new Error('This file system does not support cloning')
    }
  })
  fsMock.promises.link = jest.fn(() => {
    throw new Error('EXDEV: cross-device link not permitted')
  })
  fsMock.promises.rename = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
    },
    force: false,
    fromStore: false,
  })).toBe('copy')
  expect(fsMock.promises.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
  expect(fsMock.promises.copyFile).toBeCalledTimes(2)
})

test('packageImportMethod=hardlink: fall back to copying if hardlinking fails', async () => {
  const importPackage = createImportPackage('hardlink')
  fsMock.promises.link = jest.fn((src: string, dest: string) => {
    if (dest.endsWith('license')) {
      const err = new Error('')
      err['code'] = 'EEXIST'
      throw err
    }
    throw new Error('This file system does not support hard linking')
  })
  fsMock.promises.copyFile = jest.fn()
  expect(await importPackage('project/package', {
    filesMap: {
      'index.js': 'hash2',
      'package.json': 'hash1',
      license: 'hash3',
    },
    force: false,
    fromStore: false,
  })).toBe('hardlink')
  expect(fsMock.promises.link).toBeCalledTimes(3)
  expect(fsMock.promises.copyFile).toBeCalledTimes(2) // One time the target already exists, so it won't be copied
  expect(fsMock.promises.copyFile).toBeCalledWith(path.join('hash1'), path.join('project', '_tmp', 'package.json'))
  expect(fsMock.promises.copyFile).toBeCalledWith(path.join('hash2'), path.join('project', '_tmp', 'index.js'))
})

test('packageImportMethod=hardlink does not relink package from store if package.json is linked from the store', async () => {
  const importPackage = createImportPackage('hardlink')
  fsMock.promises.copyFile = jest.fn()
  fsMock.promises.rename = jest.fn()
  fsMock.promises.stat = jest.fn(() => ({ ino: 1 }))
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
  globalInfo.mockReset()
  const importPackage = createImportPackage('hardlink')
  fsMock.promises.copyFile = jest.fn()
  fsMock.promises.rename = jest.fn()
  let ino = 0
  fsMock.promises.stat = jest.fn(() => ({ ino: ++ino }))
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
  const importPackage = createImportPackage('hardlink')
  fsMock.promises.copyFile = jest.fn()
  fsMock.promises.rename = jest.fn()
  fsMock.promises.stat = jest.fn((file) => {
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
  globalInfo.mockReset()
  const importPackage = createImportPackage('hardlink')
  fsMock.promises.copyFile = jest.fn()
  fsMock.promises.rename = jest.fn()
  fsMock.promises.stat = jest.fn((file) => {
    if (file === path.join('project/package', 'package.json')) {
      const err = new Error()
      err['code'] = 'ENOENT'
      throw err
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
