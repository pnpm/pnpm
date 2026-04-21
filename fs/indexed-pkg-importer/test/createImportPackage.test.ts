import fs, { type BigIntStats } from 'node:fs'
import path from 'node:path'

import { afterAll, beforeEach, expect, jest, test } from '@jest/globals'
const testOnLinuxOnly = (process.platform === 'darwin' || process.platform === 'win32') ? test.skip : test

jest.unstable_mockModule('@pnpm/fs.graceful-fs', () => {
  const { access } = jest.requireActual<typeof fs>('fs')
  const fsMock = {
    access,
    copyFileSync: jest.fn(),
    readFileSync: jest.fn(),
    readdirSync: jest.fn(),
    linkSync: jest.fn(),
    mkdirSync: jest.fn(),
    renameSync: jest.fn(),
    writeFileSync: jest.fn(),
    statSync: jest.fn(),
  }
  return {
    __esModule: true,
    default: fsMock,
    ...fsMock,
  }
})
jest.unstable_mockModule('path-temp', () => ({ fastPathTemp: (file: string) => `${file}_tmp` }))
jest.unstable_mockModule('rename-overwrite', () => ({ renameOverwrite: jest.fn(), renameOverwriteSync: jest.fn() }))
jest.unstable_mockModule('fs-extra', () => ({
  default: {
    copySync: jest.fn(),
  },
}))
jest.unstable_mockModule('@pnpm/logger', () => ({
  logger: jest.fn(() => ({ debug: jest.fn() })),
  globalWarn: jest.fn(),
  globalInfo: jest.fn(),
}))

const { default: gfs } = await import('@pnpm/fs.graceful-fs')
const { createIndexedPkgImporter } = await import('@pnpm/fs.indexed-pkg-importer')
const { globalInfo } = await import('@pnpm/logger')
const { renameOverwriteSync } = await import('rename-overwrite')

beforeEach(() => {
  // Clean up real directories created by the importer (not mocked) so each
  // test starts fresh — otherwise the fast path sees leftover dirs and writes
  // over them, causing different behavior than a fresh import.
  fs.rmSync('project', { recursive: true, force: true })
  fs.rmSync('project2', { recursive: true, force: true })
  jest.mocked(gfs.copyFileSync).mockClear()
  jest.mocked(gfs.readFileSync as jest.Mock).mockClear()
  jest.mocked(gfs.writeFileSync).mockClear()
  jest.mocked(gfs.linkSync).mockClear()
  jest.mocked(gfs.mkdirSync).mockClear()
  jest.mocked(gfs.renameSync).mockClear()
  jest.mocked(gfs.statSync as jest.Mock).mockReset()
  jest.mocked(globalInfo).mockReset()
  jest.mocked(renameOverwriteSync).mockClear()
})

afterAll(() => {
  fs.rmSync('project', { recursive: true, force: true })
  fs.rmSync('project2', { recursive: true, force: true })
})

testOnLinuxOnly('packageImportMethod=auto: clone files by default', () => {
  const importPackage = createIndexedPkgImporter('auto')
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('clone')
  expect(gfs.copyFileSync).toHaveBeenCalledWith(
    path.join('hash1'),
    path.join('project', 'package', 'package.json'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
  expect(gfs.copyFileSync).toHaveBeenCalledWith(
    path.join('hash2'),
    path.join('project', 'package', 'index.js'),
    fs.constants.COPYFILE_FICLONE_FORCE
  )
})

testOnLinuxOnly('packageImportMethod=auto: link files if cloning fails', () => {
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw new Error('This file system does not support cloning')
  })
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project', 'package', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package', 'index.js'))
  expect(gfs.copyFileSync).toHaveBeenCalled()
  jest.mocked(gfs.copyFileSync).mockClear()

  // The copy function will not be called again
  expect(importPackage('project2/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.copyFileSync).not.toHaveBeenCalled()
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project2', 'package', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project2', 'package', 'index.js'))
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
    filesMap: new Map([
      ['index.js', 'hash2'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package', 'index.js'))
  expect(gfs.linkSync).toHaveBeenCalledTimes(2)
  // copyFileSync is called twice: the clone attempt fails in both the fast
  // path and the staging fallback before initialAuto moves on to hardlink.
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(2)
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
    filesMap: new Map([
      ['index.js', 'hash2'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('copy')
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package', 'index.js'))
  // 3 calls: clone fails twice (fast path + staging fallback), then copy succeeds.
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(3)
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
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
      ['license', 'hash3'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledTimes(3)
  expect(gfs.copyFileSync).toHaveBeenCalledTimes(2) // One time the target already exists, so it won't be copied
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project', 'package', 'package.json'))
  expect(gfs.copyFileSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package', 'index.js'))
})

test('packageImportMethod=hardlink does not relink package from store if package.json is linked from the store', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.statSync).mockReturnValue({ ino: BigInt(1) } as fs.BigIntStats)
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'store',
  })).toBeUndefined()
})

test('packageImportMethod=hardlink relinks package from store if package.json is not linked from the store', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  let ino = 0
  jest.mocked(gfs.statSync as jest.Mock).mockImplementation(() => ({ ino: ++ino }))
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
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
    filesMap: new Map([
      ['index.js', 'hash2'],
    ]),
    force: false,
    resolvedFrom: 'store',
  })).toBeUndefined()
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
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'store',
  })).toBe('hardlink')
  expect(globalInfo).not.toHaveBeenCalledWith('Relinking project/package from the store')
})

testOnLinuxOnly('packageImportMethod=hardlink: falls back to read+write when copyFileSync throws ENOTSUP', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.linkSync).mockImplementation(() => {
    throw new Error('hard link failed')
  })
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw Object.assign(new Error('ENOTSUP: operation not supported on socket'), { code: 'ENOTSUP' })
  })
  jest.mocked(gfs.statSync as jest.Mock).mockReturnValue({ mode: 0o644 })
  jest.mocked(gfs.readFileSync as jest.Mock).mockReturnValue(Buffer.from('file content'))
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.readFileSync).toHaveBeenCalled()
  expect(gfs.writeFileSync).toHaveBeenCalledWith(
    path.join('project', 'package', 'index.js'),
    Buffer.from('file content'),
    { mode: 0o644 }
  )
})

testOnLinuxOnly('packageImportMethod=copy: falls back to read+write when copyFileSync throws ENOTSUP', () => {
  const importPackage = createIndexedPkgImporter('copy')
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw Object.assign(new Error('ENOTSUP: operation not supported on socket'), { code: 'ENOTSUP' })
  })
  jest.mocked(gfs.statSync as jest.Mock).mockReturnValue({ mode: 0o755 })
  jest.mocked(gfs.readFileSync as jest.Mock).mockReturnValue(Buffer.from('file content'))
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('copy')
  expect(gfs.readFileSync).toHaveBeenCalled()
  expect(gfs.writeFileSync).toHaveBeenCalledWith(
    path.join('project', 'package', 'index.js'),
    Buffer.from('file content'),
    { mode: 0o755 }
  )
})

testOnLinuxOnly('packageImportMethod=auto: ENOTSUP on clone falls through to hardlinks', () => {
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation((_src, _dest, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw Object.assign(new Error('ENOTSUP: operation not supported on socket'), { code: 'ENOTSUP' })
    }
  })
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project', 'package', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project', 'package', 'index.js'))
})

testOnLinuxOnly('packageImportMethod=auto: ENOTSUP on clone uses hardlinks for all subsequent packages too', () => {
  // Regression test: the ENOTSUP fallback in createClonePkg() used to silently
  // convert clone failures to copies, so the auto-importer thought cloning
  // worked and selected it for all packages.  On ext4 (no reflink support),
  // this caused every file to be copied instead of hardlinked — a multi-second
  // regression on large projects.
  jest.mocked(gfs.linkSync).mockReset()
  const importPackage = createIndexedPkgImporter('auto')
  jest.mocked(gfs.copyFileSync).mockImplementation((_src, _dest, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw Object.assign(new Error('ENOTSUP: operation not supported on socket'), { code: 'ENOTSUP' })
    }
  })
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')

  jest.mocked(gfs.copyFileSync).mockClear()
  jest.mocked(gfs.linkSync).mockClear()

  // Second package must also use hardlinks — not copy
  expect(importPackage('project2/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('hardlink')
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash1'), path.join('project2', 'package', 'package.json'))
  expect(gfs.linkSync).toHaveBeenCalledWith(path.join('hash2'), path.join('project2', 'package', 'index.js'))
  // copyFileSync should not have been called (no clone, no copy fallback)
  expect(gfs.copyFileSync).not.toHaveBeenCalled()
})

testOnLinuxOnly('packageImportMethod=clone: falls back to copy on ENOTSUP, using atomic write for package.json', () => {
  const importPackage = createIndexedPkgImporter('clone')
  jest.mocked(gfs.copyFileSync).mockImplementation((_src, _dest, flags?: number) => {
    if (flags === fs.constants.COPYFILE_FICLONE_FORCE) {
      throw Object.assign(new Error('ENOTSUP: operation not supported on socket'), { code: 'ENOTSUP' })
    }
  })
  jest.mocked(gfs.statSync as jest.Mock).mockReturnValue({ mode: 0o644 })
  jest.mocked(gfs.readFileSync as jest.Mock).mockReturnValue(Buffer.from('file content'))
  expect(importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
      ['package.json', 'hash1'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toBe('clone')

  // Regular file: falls back to plain copyFileSync (without reflink flag)
  expect(gfs.copyFileSync).toHaveBeenCalledWith(
    path.join('hash2'),
    path.join('project', 'package', 'index.js')
  )

  // package.json: falls back to atomic temp+rename
  expect(renameOverwriteSync).toHaveBeenCalledWith(
    path.join('project', 'package', 'package.json') + '_tmp',
    path.join('project', 'package', 'package.json')
  )
})

testOnLinuxOnly('packageImportMethod=hardlink: rethrows non-ENOTSUP errors from copyFileSync', () => {
  const importPackage = createIndexedPkgImporter('hardlink')
  jest.mocked(gfs.linkSync).mockImplementation(() => {
    throw new Error('hard link failed')
  })
  jest.mocked(gfs.copyFileSync).mockImplementation(() => {
    throw Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
  })
  expect(() => importPackage('project/package', {
    filesMap: new Map([
      ['index.js', 'hash2'],
    ]),
    force: false,
    resolvedFrom: 'remote',
  })).toThrow('EACCES: permission denied')
})
