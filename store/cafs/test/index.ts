import fs from 'fs'
import path from 'path'
import symlinkDir from 'symlink-dir'
import tempy from 'tempy'
import { fixtures } from '@pnpm/test-fixtures'
import {
  createCafs,
  checkPkgFilesIntegrity,
  getFilePathByModeInCafs,
} from '../src/index.js'
import { parseTarball } from '../src/parseTarball.js'

const f = fixtures(__dirname)

describe('cafs', () => {
  it('unpack', () => {
    const dest = tempy.directory()
    const cafs = createCafs(dest)
    const { filesIndex } = cafs.addFilesFromTarball(
      fs.readFileSync(f.find('node-gyp-6.1.0.tgz'))
    )
    expect(Object.keys(filesIndex)).toHaveLength(121)
    const pkgFile = filesIndex['package.json']
    expect(pkgFile.size).toBe(1121)
    expect(pkgFile.mode).toBe(420)
    expect(typeof pkgFile.checkedAt).toBe('number')
    expect(pkgFile.integrity.toString()).toBe('sha512-8xCvrlC7W3TlwXxetv5CZTi53szYhmT7tmpXF/ttNthtTR9TC7Y7WJFPmJToHaSQ4uObuZyOARdOJYNYuTSbXA==')
  })

  it('replaces an already existing file, if the integrity of it was broken', () => {
    const storeDir = tempy.directory()
    const srcDir = path.join(__dirname, 'fixtures/one-file')
    const addFiles = () => createCafs(storeDir).addFilesFromDir(srcDir)

    let addFilesResult = addFiles()

    // Modifying the file in the store
    const filePath = getFilePathByModeInCafs(storeDir, addFilesResult.filesIndex['foo.txt'].integrity, 420)
    fs.appendFileSync(filePath, 'bar')

    addFilesResult = addFiles()
    expect(fs.readFileSync(filePath, 'utf8')).toBe('foo\n')
    expect(addFilesResult.manifest).toEqual(undefined)
  })

  it('ignores broken symlinks when traversing subdirectories', () => {
    const storeDir = tempy.directory()
    const srcDir = path.join(__dirname, 'fixtures/broken-symlink')
    const addFiles = () => createCafs(storeDir).addFilesFromDir(srcDir)

    const { filesIndex } = addFiles()
    expect(filesIndex['subdir/should-exist.txt']).toBeDefined()
  })

  it('symlinks are resolved and added as regular files', async () => {
    const storeDir = tempy.directory()
    const srcDir = tempy.directory()
    const filePath = path.join(srcDir, 'index.js')
    const symlinkPath = path.join(srcDir, 'symlink.js')
    fs.writeFileSync(filePath, '// comment', 'utf8')
    fs.symlinkSync(filePath, symlinkPath)
    fs.mkdirSync(path.join(srcDir, 'lib'))
    fs.writeFileSync(path.join(srcDir, 'lib/index.js'), '// comment 2', 'utf8')
    await symlinkDir(path.join(srcDir, 'lib'), path.join(srcDir, 'lib-symlink'))

    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)
    expect(filesIndex['symlink.js']).toBeDefined()
    expect(filesIndex['symlink.js']).toStrictEqual(filesIndex['index.js'])
    expect(filesIndex['lib/index.js']).toBeDefined()
    expect(filesIndex['lib/index.js']).toStrictEqual(filesIndex['lib-symlink/index.js'])
  })

  // Security test: symlinks pointing outside the package root should be rejected
  // This prevents file: and git: dependencies from leaking local data via malicious symlinks
  it('rejects symlinks pointing outside the package directory', () => {
    const storeDir = tempy.directory()
    const srcDir = tempy.directory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'legit.txt'), 'legitimate content')

    // Create a file outside the package that a malicious symlink tries to leak
    const outsideDir = tempy.directory()
    const secretFile = path.join(outsideDir, 'secret.txt')
    fs.writeFileSync(secretFile, 'secret content')

    // Create a symlink pointing to the file outside the package
    fs.symlinkSync(secretFile, path.join(srcDir, 'leak.txt'))

    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)

    // The legitimate file should be included
    expect(filesIndex['legit.txt']).toBeDefined()

    // The symlink pointing outside should be skipped (security fix)
    expect(filesIndex['leak.txt']).toBeUndefined()
  })

  // Security test: symlinked directories pointing outside the package should be rejected
  it('rejects symlinked directories pointing outside the package', () => {
    const storeDir = tempy.directory()
    const srcDir = tempy.directory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'legit.txt'), 'legitimate content')

    // Create a directory with secret files outside the package
    const outsideDir = tempy.directory()
    fs.writeFileSync(path.join(outsideDir, 'secret.txt'), 'secret content')

    // Create a symlink to the outside directory
    fs.symlinkSync(outsideDir, path.join(srcDir, 'leak-dir'))

    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)

    // The legitimate file should be included
    expect(filesIndex['legit.txt']).toBeDefined()

    // Files from the symlinked directory pointing outside should NOT be included
    expect(filesIndex['leak-dir/secret.txt']).toBeUndefined()
  })

  // Symlinked node_modules at the root should be skipped just like regular node_modules
  it('skips symlinked node_modules directory at root', () => {
    const storeDir = tempy.directory()
    const srcDir = tempy.directory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'index.js'), '// code')

    // Create a target directory for the symlink (inside the package to pass containment check)
    const targetDir = path.join(srcDir, '.deps')
    fs.mkdirSync(targetDir)
    fs.writeFileSync(path.join(targetDir, 'dep.js'), '// dep')

    // Create a symlinked node_modules directory at the root
    fs.symlinkSync(targetDir, path.join(srcDir, 'node_modules'))

    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)

    // The legitimate file should be included
    expect(filesIndex['index.js']).toBeDefined()
    // The target files under .deps should be included
    expect(filesIndex['.deps/dep.js']).toBeDefined()

    // Files from symlinked node_modules at root should NOT be included
    expect(filesIndex['node_modules/dep.js']).toBeUndefined()
  })
})

describe('checkPkgFilesIntegrity()', () => {
  it("doesn't fail if file was removed from the store", () => {
    const storeDir = tempy.directory()
    expect(checkPkgFilesIntegrity(storeDir, {
      files: {
        foo: {
          integrity: 'sha512-8xCvrlC7W3TlwXxetv5CZTi53szYhmT7tmpXF/ttNthtTR9TC7Y7WJFPmJToHaSQ4uObuZyOARdOJYNYuTSbXA==',
          mode: 420,
          size: 10,
        },
      },
    }).passed).toBeFalsy()
  })
})

test('file names are normalized when unpacking a tarball', () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(f.find('colorize-semver-diff.tgz'))
  )
  expect(Object.keys(filesIndex).sort()).toStrictEqual([
    'LICENSE',
    'README.md',
    'lib/index.d.ts',
    'lib/index.js',
    'package.json',
  ])
})

test('broken magic in tarball headers is handled gracefully', () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  cafs.addFilesFromTarball(
    fs.readFileSync(f.find('jquery.dirtyforms-2.0.0.tgz'))
  )
})

test('unpack an older version of tar that prefixes with spaces', () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(f.find('parsers-3.0.0-rc.48.1.tgz'))
  )
  expect(Object.keys(filesIndex).sort()).toStrictEqual([
    'lib/grammars/resolution.d.ts',
    'lib/grammars/resolution.js',
    'lib/grammars/resolution.pegjs',
    'lib/grammars/shell.d.ts',
    'lib/grammars/shell.js',
    'lib/grammars/shell.pegjs',
    'lib/grammars/syml.d.ts',
    'lib/grammars/syml.js',
    'lib/grammars/syml.pegjs',
    'lib/index.d.ts',
    'lib/index.js',
    'lib/resolution.d.ts',
    'lib/resolution.js',
    'lib/shell.d.ts',
    'lib/shell.js',
    'lib/syml.d.ts',
    'lib/syml.js',
    'package.json',
  ])
})

test('unpack a tarball that contains hard links', () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(f.find('vue.examples.todomvc.todo-store-0.0.1.tgz'))
  )
  expect(Object.keys(filesIndex).length).toBeGreaterThan(0)
})

// Regression test for Windows path traversal vulnerability
// A malicious tarball entry like "foo\..\..\..\.npmrc" should have its path normalized
test('path traversal with backslashes is blocked (Windows security fix)', () => {
  // Create a minimal valid tarball with a malicious filename
  const tarBuffer = createTarballWithEntry('foo\\..\\..\\..\\malicious.txt', 'evil content')

  const result = parseTarball(tarBuffer)
  const fileNames = Array.from(result.files.keys())

  // The path should be normalized - no ".." segments and no path traversal
  for (const fileName of fileNames) {
    expect(fileName).not.toContain('..')
    expect(fileName).not.toContain('\\')
  }
})

test.each([
  ['a directory entry', '5'],
  ['a regular file entry whose path ends in /', '0'],
])('%s with a nonzero size has no content, as in node-tar', (_, entryType) => {
  const hiddenEntry = createTarballWithEntry('index.js', 'hidden').subarray(0, -1024)
  const tarBuffer = Buffer.concat([
    createTarballWithEntry('index.js', 'benign').subarray(0, -1024),
    createTarHeader('package/docs/', 512, entryType),
    createTarHeader('package/NOTES', hiddenEntry.length),
    hiddenEntry,
    Buffer.alloc(1024),
  ])

  const { files } = parseTarball(tarBuffer)

  expect([...files.keys()]).toStrictEqual(['index.js', 'NOTES'])
  expect(readParsedFile(tarBuffer, files.get('index.js'))).toBe('benign')
  expect(files.get('NOTES')?.size).toBe(hiddenEntry.length)
})

test('a regular file entry whose PAX path ends in / has no content, as in node-tar', () => {
  const paxPayload = Buffer.alloc(512)
  paxPayload.write('22 path=package/docs/\n')
  const nextEntry = createTarballWithEntry('index.js', 'module.exports = 1')
  const tarBuffer = Buffer.concat([
    createTarHeader('PaxHeader/docs', 22, 'x'),
    paxPayload,
    createTarHeader('package/docs', nextEntry.length - 1024),
    nextEntry,
  ])

  const { files } = parseTarball(tarBuffer)

  expect([...files.keys()]).toStrictEqual(['index.js'])
  expect(readParsedFile(tarBuffer, files.get('index.js'))).toBe('module.exports = 1')
})

function readParsedFile (tarBuffer: Buffer, file: { offset: number, size: number } | undefined): string | undefined {
  return file && tarBuffer.toString('utf8', file.offset, file.offset + file.size)
}

// Helper to create a minimal tarball buffer with a single entry
function createTarballWithEntry (fileName: string, content: string): Buffer {
  const contentBytes = Buffer.from(content, 'utf8')
  const header = createTarHeader(`package/${fileName}`, contentBytes.length)

  // Content block (padded to 512 bytes)
  const contentBlock = Buffer.alloc(512, 0)
  contentBytes.copy(contentBlock)

  // End-of-archive marker (two 512-byte blocks of zeros)
  const endMarker = Buffer.alloc(1024, 0)

  return Buffer.concat([header, contentBlock, endMarker])
}

function createTarHeader (entryPath: string, size: number, entryType = '0'): Buffer {
  const header = Buffer.alloc(512, 0)
  header.write(entryPath, 0, Math.min(entryPath.length, 100), 'utf8')
  header.write('0000644\0', 100, 8, 'utf8')
  header.write('0000000\0', 108, 8, 'utf8')
  header.write('0000000\0', 116, 8, 'utf8')
  header.write(size.toString(8).padStart(11, '0') + '\0', 124, 12, 'utf8')
  header.write('00000000000\0', 136, 12, 'utf8')
  header[156] = entryType.charCodeAt(0)
  header.write('ustar\0', 257, 6, 'utf8')
  header.write('00', 263, 2, 'utf8')
  // The checksum is the sum of all header bytes, counting the checksum field as spaces.
  header.fill(' ', 148, 156)
  let checksum = 0
  for (const byte of header) {
    checksum += byte
  }
  header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'utf8')
  return header
}

// Related issue: https://github.com/pnpm/pnpm/issues/7120
test('unpack should not fail when the tarball format seems to be not USTAR or GNU TAR', () => {
  const dest = tempy.directory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(f.find('devextreme-17.1.6.tgz'))
  )
  expect(Object.keys(filesIndex).length).toBeGreaterThan(0)
})
