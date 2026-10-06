import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { gunzipSync, gzipSync } from 'node:zlib'

import { describe, expect, it, jest, test } from '@jest/globals'
import { fixtures } from '@pnpm/test-fixtures'
import { symlinkDir } from 'symlink-dir'
import { temporaryDirectory } from 'tempy'

import { MAX_IN_MEMORY_TARBALL_SIZE } from '../src/addFilesFromTarball.js'
import {
  checkPkgFilesIntegrity,
  createCafs,
  getFilePathByModeInCafs,
} from '../src/index.js'
import { createTarballParser, paddingOf } from '../src/parseTarball.js'

const testFixtures = fixtures(import.meta.dirname)

describe('cafs', () => {
  it('unpack', () => {
    const dest = temporaryDirectory()
    const cafs = createCafs(dest)
    const { filesIndex } = cafs.addFilesFromTarball(
      fs.readFileSync(testFixtures.find('node-gyp-6.1.0.tgz'))
    )
    expect(filesIndex.size).toBe(121)
    const pkgFile = filesIndex.get('package.json')
    expect(pkgFile!.size).toBe(1121)
    expect(pkgFile!.mode).toBe(420)
    expect(typeof pkgFile!.checkedAt).toBe('number')
    expect(pkgFile!.digest).toBe('f310afae50bb5b74e5c17c5eb6fe426538b9deccd88664fbb66a5717fb6d36d86d4d1f530bb63b58914f9894e81da490e2e39bb99c8e01174e258358b9349b5c')
  })

  it('unpack bzip2 tarball', () => {
    const dest = temporaryDirectory()
    const cafs = createCafs(dest)
    const { filesIndex, manifest } = cafs.addFilesFromTarball(
      fs.readFileSync(path.join(import.meta.dirname, 'fixtures/package.tar.bz2')),
      true
    )
    expect(Array.from(filesIndex.keys()).sort()).toStrictEqual(['index.js', 'package.json'])
    expect(manifest?.name).toBe('test-bzip2-pkg')
    expect(manifest?.version).toBe('1.2.3')
  })

  it('addFilesFromTarball honors a per-call ignore predicate', () => {
    const dest = temporaryDirectory()
    const cafs = createCafs(dest)
    const tarball = fs.readFileSync(testFixtures.find('node-gyp-6.1.0.tgz'))
    const baseline = cafs.addFilesFromTarball(tarball)
    const filtered = cafs.addFilesFromTarball(tarball, false, (name) => name === 'package.json')
    expect(filtered.filesIndex.has('package.json')).toBe(false)
    expect(filtered.filesIndex.size).toBe(baseline.filesIndex.size - 1)
  })

  it('addFilesFromTarball combines cafs-level ignoreFile with per-call ignore', () => {
    const dest = temporaryDirectory()
    const cafs = createCafs(dest, { ignoreFile: (name) => name === 'package.json' })
    const tarball = fs.readFileSync(testFixtures.find('node-gyp-6.1.0.tgz'))
    const { filesIndex } = cafs.addFilesFromTarball(tarball, false, (name) => name === 'README.md')
    expect(filesIndex.has('package.json')).toBe(false)
    expect(filesIndex.has('README.md')).toBe(false)
  })

  it('replaces an already existing file, if the integrity of it was broken', () => {
    const storeDir = temporaryDirectory()
    const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
    const addFiles = () => createCafs(storeDir).addFilesFromDir(srcDir)

    let addFilesResult = addFiles()

    // Modifying the file in the store
    const { digest } = addFilesResult.filesIndex.get('foo.txt')!
    const filePath = getFilePathByModeInCafs(storeDir, digest, 420)
    fs.appendFileSync(filePath, 'bar')

    addFilesResult = addFiles()
    expect(fs.readFileSync(filePath, 'utf8')).toBe('foo\n')
    expect(addFilesResult.manifest).toBeUndefined()
  })

  it('ignores broken symlinks when traversing subdirectories', () => {
    const storeDir = temporaryDirectory()
    const srcDir = testFixtures.prepare('broken-symlink')
    fs.symlinkSync('../dangling', path.join(srcDir, 'dangling'))
    const addFiles = () => createCafs(storeDir).addFilesFromDir(srcDir)

    const { filesIndex } = addFiles()
    expect(filesIndex.get('subdir/should-exist.txt')).toBeDefined()
  })

  it('symlinks are resolved and added as regular files', async () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()
    const filePath = path.join(srcDir, 'index.js')
    const symlinkPath = path.join(srcDir, 'symlink.js')
    fs.writeFileSync(filePath, '// comment', 'utf8')
    fs.symlinkSync(filePath, symlinkPath)
    fs.mkdirSync(path.join(srcDir, 'lib'))
    fs.writeFileSync(path.join(srcDir, 'lib/index.js'), '// comment 2', 'utf8')
    await symlinkDir(path.join(srcDir, 'lib'), path.join(srcDir, 'lib-symlink'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir)
    expect(hasUnrecordedSymlinks).toBe(true)
    expect(filesIndex.get('symlink.js')).toBeDefined()
    expect(filesIndex.get('symlink.js')).toStrictEqual(filesIndex.get('index.js'))
    expect(filesIndex.get('lib/index.js')).toBeDefined()
    expect(filesIndex.get('lib/index.js')).toStrictEqual(filesIndex.get('lib-symlink/index.js'))
  })

  // Security test: symlinks pointing outside the package root should be rejected
  // This prevents file: and git: dependencies from leaking local data via malicious symlinks
  it('rejects symlinks pointing outside the package directory', () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'legit.txt'), 'legitimate content')

    // Create a file outside the package that a malicious symlink tries to leak
    const outsideDir = temporaryDirectory()
    const secretFile = path.join(outsideDir, 'secret.txt')
    fs.writeFileSync(secretFile, 'secret content')

    // Create a symlink pointing to the file outside the package
    fs.symlinkSync(secretFile, path.join(srcDir, 'leak.txt'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir)
    expect(hasUnrecordedSymlinks).toBe(true)

    // The legitimate file should be included
    expect(filesIndex.get('legit.txt')).toBeDefined()

    // The symlink pointing outside should be skipped (security fix)
    expect(filesIndex.get('leak.txt')).toBeUndefined()
  })

  // Security test: symlinked directories pointing outside the package should be rejected
  it('rejects symlinked directories pointing outside the package', () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'legit.txt'), 'legitimate content')

    // Create a directory with secret files outside the package
    const outsideDir = temporaryDirectory()
    fs.writeFileSync(path.join(outsideDir, 'secret.txt'), 'secret content')

    // Create a symlink to the outside directory
    fs.symlinkSync(outsideDir, path.join(srcDir, 'leak-dir'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir)
    expect(hasUnrecordedSymlinks).toBe(true)

    // The legitimate file should be included
    expect(filesIndex.get('legit.txt')).toBeDefined()

    // Files from the symlinked directory pointing outside should NOT be included
    expect(filesIndex.get('leak-dir/secret.txt')).toBeUndefined()
  })

  // Symlinked node_modules at the root should be skipped just like regular node_modules
  it('skips symlinked node_modules directory at root', async () => {
    const storeDir = temporaryDirectory()
    const srcDir = temporaryDirectory()

    // Create a legitimate file inside the package
    fs.writeFileSync(path.join(srcDir, 'index.js'), '// code')

    // Create a target directory for the symlink (inside the package to pass containment check)
    const targetDir = path.join(srcDir, '.deps')
    fs.mkdirSync(targetDir)
    fs.writeFileSync(path.join(targetDir, 'dep.js'), '// dep')

    // Create a symlinked node_modules directory at the root
    await symlinkDir(targetDir, path.join(srcDir, 'node_modules'))

    const { filesIndex, hasUnrecordedSymlinks } = createCafs(storeDir).addFilesFromDir(srcDir)
    expect(hasUnrecordedSymlinks).toBe(false)

    // The legitimate file should be included
    expect(filesIndex.get('index.js')).toBeDefined()
    // The target files under .deps should be included
    expect(filesIndex.get('.deps/dep.js')).toBeDefined()

    // Files from symlinked node_modules at root should NOT be included
    expect(filesIndex.get('node_modules/dep.js')).toBeUndefined()
  })
})

describe('checkPkgFilesIntegrity()', () => {
  it("doesn't fail if file was removed from the store", () => {
    const storeDir = temporaryDirectory()
    expect(checkPkgFilesIntegrity(storeDir, {
      algo: 'sha512',
      files: new Map([
        ['foo', {
          digest: 'f310afae50bb5b74e5c17c5eb6fe426538b9deccd88664fbb66a5717fb6d36d86d4d1f530bb63b58914f9894e81da490e2e39bb99c8e01174e258358b9349b5c',
          mode: 420,
          size: 10,
        }],
      ]),
    }).passed).toBeFalsy()
  })

  it('leaves a mismatched file in place for the re-fetch to replace atomically', () => {
    const storeDir = temporaryDirectory()
    const actual = Buffer.from('actual bytes!!!')
    const claimedDigest = crypto.createHash('sha512').update('claimed content').digest('hex')
    const filename = getFilePathByModeInCafs(storeDir, claimedDigest, 420)
    fs.mkdirSync(path.dirname(filename), { recursive: true })
    fs.writeFileSync(filename, actual)
    expect(checkPkgFilesIntegrity(storeDir, {
      algo: 'sha512',
      files: new Map([
        ['foo', { digest: claimedDigest, mode: 420, size: actual.length }],
      ]),
    }).passed).toBeFalsy()
    // A concurrent install sharing the store may be importing from this
    // path right now; the re-fetch replaces it via temp+rename instead.
    expect(fs.existsSync(filename)).toBeTruthy()
  })

  // Windows ignores the 0o000 mode, and root reads through it, so the open
  // cannot be made to fail in either case.
  const itOnPosix = process.platform === 'win32' || process.getuid?.() === 0 ? it.skip : it
  itOnPosix('leaves an unreadable file in place', () => {
    const storeDir = temporaryDirectory()
    const content = Buffer.from('guarded content')
    const digest = crypto.createHash('sha512').update(content).digest('hex')
    const filename = getFilePathByModeInCafs(storeDir, digest, 420)
    fs.mkdirSync(path.dirname(filename), { recursive: true })
    fs.writeFileSync(filename, content)
    fs.chmodSync(filename, 0)
    // A non-ENOENT read failure surfaces as an error (graceful-fs already
    // absorbs transient EMFILE pressure); what matters here is that the
    // blob is not deleted on the way out.
    expect(() => checkPkgFilesIntegrity(storeDir, {
      algo: 'sha512',
      files: new Map([
        ['foo', { digest, mode: 420, size: content.length }],
      ]),
    })).toThrow()
    fs.chmodSync(filename, 0o644)
    expect(fs.existsSync(filename)).toBeTruthy()
  })

  it('removes a directory squatting at a blob path so the re-fetch can land', () => {
    const storeDir = temporaryDirectory()
    const claimedDigest = crypto.createHash('sha512').update('some content').digest('hex')
    const filename = getFilePathByModeInCafs(storeDir, claimedDigest, 420)
    fs.mkdirSync(filename, { recursive: true })
    expect(checkPkgFilesIntegrity(storeDir, {
      algo: 'sha512',
      files: new Map([
        ['foo', { digest: claimedDigest, mode: 420, size: 1_000_000 }],
      ]),
    }).passed).toBeFalsy()
    expect(fs.existsSync(filename)).toBeFalsy()
  })
})

test('file names are normalized when unpacking a tarball', () => {
  const dest = temporaryDirectory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(testFixtures.find('colorize-semver-diff.tgz'))
  )
  expect(Array.from(filesIndex.keys()).sort()).toStrictEqual([
    'LICENSE',
    'README.md',
    'lib/index.d.ts',
    'lib/index.js',
    'package.json',
  ])
})

test('broken magic in tarball headers is handled gracefully', () => {
  const dest = temporaryDirectory()
  const cafs = createCafs(dest)
  cafs.addFilesFromTarball(
    fs.readFileSync(testFixtures.find('jquery.dirtyforms-2.0.0.tgz'))
  )
})

test('unpack an older version of tar that prefixes with spaces', () => {
  const dest = temporaryDirectory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(testFixtures.find('parsers-3.0.0-rc.48.1.tgz'))
  )
  expect(Array.from(filesIndex.keys()).sort()).toStrictEqual([
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
  const dest = temporaryDirectory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(testFixtures.find('vue.examples.todomvc.todo-store-0.0.1.tgz'))
  )
  expect(filesIndex.size).toBeGreaterThan(0)
})

// Regression test for Windows path traversal vulnerability
// A malicious tarball entry like "foo\..\..\..\.npmrc" should have its path normalized
test('path traversal with backslashes is blocked (Windows security fix)', () => {
  // Create a minimal valid tarball with a malicious filename
  const tarBuffer = createTarballWithEntry('package/foo\\..\\..\\..\\malicious.txt', 'evil content')

  const fileNames = Array.from(parseTarballEntries(tarBuffer).keys())

  // The path should be normalized - no ".." segments and no path traversal
  for (const fileName of fileNames) {
    expect(fileName).not.toContain('..')
    expect(fileName).not.toContain('\\')
  }
})

test('only one segment is stripped from a dot-prefixed tarball entry', () => {
  const entries = parseTarballEntries(createTarballWithEntry('./package/package.json', '{}'))

  expect(Array.from(entries.keys())).toStrictEqual(['package/package.json'])
})

test.each([
  'node-gyp-6.1.0.tgz',
  'colorize-semver-diff.tgz',
  'parsers-3.0.0-rc.48.1.tgz',
  'vue.examples.todomvc.todo-store-0.0.1.tgz',
  'devextreme-17.1.6.tgz',
  'long-paths-gnu.tgz',
  'long-paths-pax.tgz',
])('a tarball fed in small chunks parses the same as a whole one: %s', (fixture) => {
  const tarContent = gunzipSync(fs.readFileSync(findTarballFixture(fixture)))
  const whole = parseTarballEntries(tarContent)
  for (const chunkSize of [1, 511, 513, 4096]) {
    expect(parseTarballEntries(tarContent, chunkSize)).toStrictEqual(whole)
  }
})

test.each(['long-paths-gnu.tgz', 'long-paths-pax.tgz'])('paths longer than a TAR header field are read from %s', (fixture) => {
  const entries = parseTarballEntries(gunzipSync(fs.readFileSync(findTarballFixture(fixture))))
  expect(Array.from(entries.keys()).sort()).toStrictEqual([
    `${'a'.repeat(60)}/${'b'.repeat(60)}/${'c'.repeat(40)}.js`,
    'package.json',
  ])
})

test('a truncated tarball is rejected', () => {
  const tarContent = createTarballWithEntry('package/index.js', 'x'.repeat(1000))
  expect(() => parseTarballEntries(tarContent.subarray(0, 1024))).toThrow('Unexpected end of TAR archive at offset 1024')
})

test('an entry is not allocated from its declared size before its content arrives', () => {
  const declaredSize = 4 * 1024 * 1024 * 1024
  const header = createTarballWithEntry('package/index.js', '', { declaredSize }).subarray(0, 512)
  const parser = createTarballParser(() => {})
  const arrayBuffersBefore = process.memoryUsage().arrayBuffers
  parser.push(header)
  parser.push(Buffer.alloc(1024, 'x'))
  expect(process.memoryUsage().arrayBuffers - arrayBuffersBefore).toBeLessThan(declaredSize / 4)
  expect(() => parser.end()).toThrow('Unexpected end of TAR archive at offset 1536')
})

test('an entry whose size is not a number is rejected', () => {
  const tarContent = createTarballWithEntry('package/index.js', '', { rawSizeField: 'zzzzzzzzzzz' })
  expect(() => parseTarballEntries(tarContent)).toThrow('Invalid file size for TAR header at offset 0')
})

test('paddingOf computes correct block padding without 32-bit truncation', () => {
  expect(paddingOf(0)).toBe(0)
  expect(paddingOf(512)).toBe(0)
  expect(paddingOf(1)).toBe(511)
  expect(paddingOf(511)).toBe(1)
  expect(paddingOf(513)).toBe(511)
  const fourGib = 4 * 1024 * 1024 * 1024
  expect(paddingOf(fourGib)).toBe(0)
  expect(paddingOf(fourGib + 1)).toBe(511)
  expect(paddingOf(fourGib + 511)).toBe(1)
  expect(paddingOf(fourGib + 512)).toBe(0)
  expect(paddingOf(fourGib + 513)).toBe(511)
})

test('rejects PAX header with negative length', () => {
  const tarContent = createTarballWithPaxHeader('-12 path=bad\n')
  expect(() => parseTarballEntries(tarContent)).toThrow('Invalid length in PAX record: -12')
})

test('rejects PAX header with length exceeding buffer', () => {
  const tarContent = createTarballWithPaxHeader('999999 path=bad\n')
  expect(() => parseTarballEntries(tarContent)).toThrow('Invalid length in PAX record: 999999')
})

test('rejects PAX header without space delimiter', () => {
  const tarContent = createTarballWithPaxHeader('1234567890')
  expect(() => parseTarballEntries(tarContent)).toThrow('Invalid PAX record format: missing space delimiter')
})

test('rejects PAX header without newline terminator', () => {
  const tarContent = createTarballWithPaxHeader('12 path=badX')
  expect(() => parseTarballEntries(tarContent)).toThrow('Invalid PAX record format: missing newline terminator')
})

test('rejects PAX header with invalid or negative size', () => {
  const negativeSizePax = createTarballWithPaxHeader('14 size=-1000\n')
  expect(() => parseTarballEntries(negativeSizePax)).toThrow('Invalid size in PAX record: size=-1000')
})

test('nothing is written to the store from a malformed archive', () => {
  const storeDir = temporaryDirectory()
  const validEntry = createTarballWithEntry('package/index.js', 'module.exports = 1').subarray(0, 1024)
  const brokenHeader = Buffer.alloc(512, 'x')
  expect(() => createCafs(storeDir).addFilesFromTarball(gzipSync(Buffer.concat([validEntry, brokenHeader, Buffer.alloc(1024)]))))
    .toThrow('Invalid checksum for TAR header at offset 1024')
  expect(fs.readdirSync(storeDir)).toStrictEqual([])
})

describe('addFilesFromTarballBounded', () => {
  // All-zero content compresses to a small gzip body.
  const largeFileSize = MAX_IN_MEMORY_TARBALL_SIZE + 1024 * 1024
  const largeFileDigest = crypto.hash('sha512', Buffer.alloc(largeFileSize), 'hex')
  let largeTarball: Buffer
  function getLargeTarball (): Buffer {
    largeTarball ??= gzipSync(Buffer.concat([
      createTarballWithEntry('package/package.json', '{"name":"large","version":"1.0.0"}').subarray(0, 1024),
      createTarballWithEntry('package/zeros.bin', Buffer.alloc(largeFileSize)),
    ]))
    return largeTarball
  }

  it('extracts a small gzip archive in memory', async () => {
    const tarball = fs.readFileSync(testFixtures.find('node-gyp-6.1.0.tgz'))
    const expected = createCafs(temporaryDirectory()).addFilesFromTarball(tarball, true)
    const actual = await createCafs(temporaryDirectory()).addFilesFromTarballBounded(tarball, true)
    expect(digestsOf(actual.filesIndex)).toStrictEqual(digestsOf(expected.filesIndex))
    expect(actual.manifest).toStrictEqual(expected.manifest)
  })

  it('streams a gzip archive larger than the in-memory limit', async () => {
    const { filesIndex, manifest } = await createCafs(temporaryDirectory()).addFilesFromTarballBounded(getLargeTarball(), true)
    expect(manifest?.name).toBe('large')
    expect(filesIndex.get('zeros.bin')).toMatchObject({ size: largeFileSize, digest: largeFileDigest })
  })

  it('writes a large file without assembling its content in memory', async () => {
    const tarball = getLargeTarball()
    const concat = Buffer.concat
    const spy = jest.spyOn(Buffer, 'concat').mockImplementation((buffers, size) => {
      expect(size ?? buffers.reduce((sum, buffer) => sum + buffer.length, 0)).toBeLessThanOrEqual(MAX_IN_MEMORY_TARBALL_SIZE)
      return concat(buffers, size)
    })
    const storeDir = temporaryDirectory()
    try {
      const { filesIndex } = await createCafs(storeDir).addFilesFromTarballBounded(tarball)
      expect(filesIndex.get('zeros.bin')).toMatchObject({ size: largeFileSize, digest: largeFileDigest })
      expect(fs.readdirSync(storeDir)).toEqual(['files'])
    } finally {
      spy.mockRestore()
    }
  })

  it('extracts a file-backed gzip archive with the same file digests and manifest', async () => {
    const tarballFile = path.join(temporaryDirectory(), 'archive.tgz')
    const tarball = fs.readFileSync(testFixtures.find('node-gyp-6.1.0.tgz'))
    fs.writeFileSync(tarballFile, tarball)
    const expected = createCafs(temporaryDirectory()).addFilesFromTarball(tarball, true)
    const actual = await createCafs(temporaryDirectory()).addFilesFromTarballFile(tarballFile, true)
    expect(digestsOf(actual.filesIndex)).toStrictEqual(digestsOf(expected.filesIndex))
    expect(actual.manifest).toStrictEqual(expected.manifest)
  })

  it('removes an unfinished large file when a file-backed archive is truncated', async () => {
    const tarballFile = path.join(temporaryDirectory(), 'truncated.tgz')
    const header = createTarballWithEntry('package/large', '', { declaredSize: largeFileSize }).subarray(0, 512)
    fs.writeFileSync(tarballFile, gzipSync(Buffer.concat([header, Buffer.alloc(1024)])))
    const storeDir = temporaryDirectory()
    await expect(createCafs(storeDir).addFilesFromTarballFile(tarballFile)).rejects.toThrow('Unexpected end of TAR archive')
    expect(fs.readdirSync(storeDir)).toEqual([])
  })

  test.each(['package/package.json', 'package/metadata'])('rejects an oversized buffered entry %s before reading its content', async (entryPath) => {
    const tarballFile = path.join(temporaryDirectory(), 'archive.tgz')
    const header = createTarballWithEntry(entryPath, '', { declaredSize: largeFileSize }).subarray(0, 512)
    if (entryPath === 'package/metadata') {
      header[156] = 'x'.charCodeAt(0)
      header.fill(0x20, 148, 156)
      const checksum = header.reduce((sum, value) => sum + value, 0)
      header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8)
    }
    fs.writeFileSync(tarballFile, gzipSync(Buffer.concat([header, Buffer.alloc(1024)])))
    await expect(createCafs(temporaryDirectory()).addFilesFromTarballFile(tarballFile, true))
      .rejects.toMatchObject({ code: 'ERR_PNPM_TARBALL_ENTRY_TOO_LARGE' })
  })

  it('reuses and repairs a streamed CAS file without breaking project hardlinks', async () => {
    const storeDir = temporaryDirectory()
    const cafs = createCafs(storeDir)
    const first = await cafs.addFilesFromTarballBounded(getLargeTarball())
    const filePath = first.filesIndex.get('zeros.bin')!.filePath
    const projectFile = path.join(temporaryDirectory(), 'linked-file')
    fs.linkSync(filePath, projectFile)
    const originalInode = fs.statSync(filePath).ino

    await cafs.addFilesFromTarballBounded(getLargeTarball())
    expect(fs.statSync(filePath).ino).toBe(originalInode)
    expect(fs.statSync(projectFile).ino).toBe(originalInode)

    fs.writeFileSync(filePath, 'corrupt')
    await cafs.addFilesFromTarballBounded(getLargeTarball())
    expect(fs.statSync(filePath).ino).toBe(originalInode)
    expect(fs.statSync(projectFile).size).toBe(largeFileSize)
    expect(crypto.hash('sha512', fs.readFileSync(projectFile), 'hex')).toBe(largeFileDigest)
  })

  it('extracts bzip2 archives from a buffer and a file with matching digests', async () => {
    const tarballFile = path.join(import.meta.dirname, 'fixtures/package.tar.bz2')
    const tarball = fs.readFileSync(tarballFile)
    const expected = createCafs(temporaryDirectory()).addFilesFromTarball(tarball, true)
    const buffered = await createCafs(temporaryDirectory()).addFilesFromTarballBounded(tarball, true)
    const cloned = await createCafs(temporaryDirectory()).addFilesFromTarballBounded(structuredClone(tarball), true)
    const fromFile = await createCafs(temporaryDirectory()).addFilesFromTarballFile(tarballFile, true)
    expect(digestsOf(buffered.filesIndex)).toStrictEqual(digestsOf(expected.filesIndex))
    expect(digestsOf(cloned.filesIndex)).toStrictEqual(digestsOf(expected.filesIndex))
    expect(digestsOf(fromFile.filesIndex)).toStrictEqual(digestsOf(expected.filesIndex))
    expect(buffered.manifest).toStrictEqual(expected.manifest)
    expect(cloned.manifest).toStrictEqual(expected.manifest)
    expect(fromFile.manifest).toStrictEqual(expected.manifest)
  })

  test.each([false, true])('streams a large bzip2 payload with file-backed input %s', async (fileBacked) => {
    const tarballFile = path.join(import.meta.dirname, 'fixtures/large-zero-file.tar.bz2')
    const cafs = createCafs(temporaryDirectory())
    const result = fileBacked
      ? await cafs.addFilesFromTarballFile(tarballFile)
      : await cafs.addFilesFromTarballBounded(fs.readFileSync(tarballFile))
    expect(result.filesIndex.get('zeros.bin')).toMatchObject({ size: largeFileSize, digest: largeFileDigest })
  })

  test.each([
    ['large-manifest.tar.bz2', false], ['large-manifest.tar.bz2', true],
    ['large-metadata.tar.bz2', false], ['large-metadata.tar.bz2', true],
  ])('bzip2 buffering limits reject %s with file-backed input %s', async (fixture, fileBacked) => {
    const tarballFile = path.join(import.meta.dirname, 'fixtures', fixture)
    const cafs = createCafs(temporaryDirectory())
    const result = fileBacked
      ? cafs.addFilesFromTarballFile(tarballFile, true)
      : cafs.addFilesFromTarballBounded(fs.readFileSync(tarballFile), true)
    await expect(result).rejects.toMatchObject({ code: 'ERR_PNPM_TARBALL_ENTRY_TOO_LARGE' })
  })

  it('streams a multi-member gzip archive whose last trailer understates its size', async () => {
    const largeTar = gunzipSync(getLargeTarball())
    const splitAt = largeTar.length - 1024
    const tarball = Buffer.concat([gzipSync(largeTar.subarray(0, splitAt)), gzipSync(largeTar.subarray(splitAt))])
    const { filesIndex } = await createCafs(temporaryDirectory()).addFilesFromTarballBounded(tarball)
    expect(filesIndex.get('zeros.bin')).toMatchObject({ size: largeFileSize, digest: largeFileDigest })
  })

  it('rejects a corrupt gzip archive larger than the in-memory limit', async () => {
    const tarball = Buffer.from(getLargeTarball())
    tarball.fill(0xff, 10, 100)
    await expect(createCafs(temporaryDirectory()).addFilesFromTarballBounded(tarball)).rejects.toThrow()
  })
})

function findTarballFixture (name: string): string {
  const localFixture = path.join(import.meta.dirname, 'fixtures', name)
  return fs.existsSync(localFixture) ? localFixture : testFixtures.find(name)
}

function digestsOf (filesIndex: Map<string, { digest: string }>): Record<string, string> {
  return Object.fromEntries(Array.from(filesIndex, ([name, { digest }]) => [name, digest]))
}

function parseTarballEntries (tarContent: Buffer, chunkSize = tarContent.length): Map<string, { mode: number, digest: string }> {
  const entries = new Map<string, { mode: number, digest: string }>()
  const parser = createTarballParser((relativePath, mode, content) => {
    entries.set(relativePath, { mode, digest: crypto.hash('sha512', content, 'hex') })
  })
  for (let offset = 0; offset < tarContent.length; offset += chunkSize) {
    parser.push(tarContent.subarray(offset, offset + chunkSize))
  }
  parser.end()
  return entries
}

function createTarballWithEntry (
  entryPath: string,
  content: string | Buffer,
  { declaredSize, rawSizeField }: { declaredSize?: number, rawSizeField?: string } = {}
): Buffer {
  const contentBytes = typeof content === 'string' ? Buffer.from(content, 'utf8') : content

  // Create a 512-byte header
  const header = Buffer.alloc(512, 0)

  // File name at offset 0 (max 100 chars)
  header.write(entryPath, 0, Math.min(entryPath.length, 100), 'utf8')

  // File mode at offset 100 (octal, 8 bytes) - 0644
  header.write('0000644\0', 100, 8, 'utf8')

  // UID at offset 108 (octal, 8 bytes)
  header.write('0000000\0', 108, 8, 'utf8')

  // GID at offset 116 (octal, 8 bytes)
  header.write('0000000\0', 116, 8, 'utf8')

  // File size at offset 124 (octal, 12 bytes)
  const sizeOctal = rawSizeField ?? (declaredSize ?? contentBytes.length).toString(8).padStart(11, '0')
  header.write(sizeOctal + '\0', 124, 12, 'utf8')

  // Mtime at offset 136 (octal, 12 bytes)
  header.write('00000000000\0', 136, 12, 'utf8')

  // File type at offset 156 ('0' for regular file)
  header[156] = '0'.charCodeAt(0)

  // USTAR indicator at offset 257
  header.write('ustar\0', 257, 6, 'utf8')
  header.write('00', 263, 2, 'utf8')

  // Compute checksum (offset 148, 8 bytes) - sum of all header bytes treating checksum field as spaces
  // First, fill checksum field with spaces
  header.fill(' ', 148, 156)
  let checksum = 0
  for (const byte of header) {
    checksum += byte
  }
  const checksumOctal = checksum.toString(8).padStart(6, '0')
  header.write(checksumOctal + '\0 ', 148, 8, 'utf8')

  // Content blocks (padded to a multiple of 512 bytes)
  const contentBlock = Buffer.alloc(Math.ceil(contentBytes.length / 512) * 512, 0)
  contentBytes.copy(contentBlock)

  // End-of-archive marker (two 512-byte blocks of zeros)
  const endMarker = Buffer.alloc(1024, 0)

  return Buffer.concat([header, contentBlock, endMarker])
}

function createTarballWithPaxHeader (
  paxRecord: string | Buffer,
  entryPath: string = 'package/index.js',
  entryContent: string = 'module.exports = 1'
): Buffer {
  const paxPayload = typeof paxRecord === 'string' ? Buffer.from(paxRecord, 'utf8') : paxRecord
  const paxBlock = Buffer.alloc(Math.ceil(paxPayload.length / 512) * 512, 0)
  paxPayload.copy(paxBlock)

  const paxHeader = Buffer.alloc(512, 0)
  paxHeader.write('PaxHeader/test', 0, 14, 'utf8')
  paxHeader.write('0000644\0', 100, 8, 'utf8')
  paxHeader.write('0000000\0', 108, 8, 'utf8')
  paxHeader.write('0000000\0', 116, 8, 'utf8')
  paxHeader.write(paxPayload.length.toString(8).padStart(11, '0') + '\0', 124, 12, 'utf8')
  paxHeader.write('00000000000\0', 136, 12, 'utf8')
  paxHeader[156] = 'x'.charCodeAt(0)
  paxHeader.write('ustar\0', 257, 6, 'utf8')
  paxHeader.write('00', 263, 2, 'utf8')
  paxHeader.fill(' ', 148, 156)
  let checksum = 0
  for (const byte of paxHeader) {
    checksum += byte
  }
  paxHeader.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'utf8')

  const normalEntry = createTarballWithEntry(entryPath, entryContent)
  return Buffer.concat([paxHeader, paxBlock, normalEntry])
}

// Related issue: https://github.com/pnpm/pnpm/issues/7120
const testOnPosix = process.platform === 'win32' ? test.skip : test

testOnPosix('files added to a group-writable store keep group write and a second add keeps the inode', () => {
  const parent = temporaryDirectory()
  fs.chmodSync(parent, 0o2775)
  const storeDir = path.join(parent, 'store')
  const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
  const first = createCafs(storeDir).addFilesFromDir(srcDir)
  const info = first.filesIndex.get('foo.txt')!
  const filePath = getFilePathByModeInCafs(storeDir, info.digest, info.mode)
  const stat = fs.statSync(filePath)
  expect(stat.mode & 0o020).not.toBe(0)
  expect(stat.gid).toBe(fs.statSync(parent).gid)

  const second = createCafs(storeDir).addFilesFromDir(srcDir)
  const again = second.filesIndex.get('foo.txt')!
  const after = fs.statSync(getFilePathByModeInCafs(storeDir, again.digest, again.mode))
  expect(after.ino).toBe(stat.ino)
  expect(after.uid).toBe(stat.uid)
  expect(after.gid).toBe(stat.gid)
  expect(after.mode & 0o777).toBe(stat.mode & 0o777)
})

testOnPosix('directories added to a group-writable store stay searchable by the group under a restrictive umask', () => {
  const parent = temporaryDirectory()
  fs.chmodSync(parent, 0o2775)
  const storeDir = path.join(parent, 'store')
  const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
  const previousUmask = process.umask(0o077)
  let filePath: string
  try {
    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)
    const info = filesIndex.get('foo.txt')!
    filePath = getFilePathByModeInCafs(storeDir, info.digest, info.mode)
  } finally {
    process.umask(previousUmask)
  }
  expect(fs.statSync(filePath).mode & 0o060).toBe(0o060)
  for (let dir = path.dirname(filePath); dir !== parent; dir = path.dirname(dir)) {
    expect(fs.statSync(dir).mode & 0o2070).toBe(0o2070)
  }
})

testOnPosix('files added to a world-writable sticky store are not world-writable', () => {
  const parent = temporaryDirectory()
  fs.chmodSync(parent, 0o1777)
  const storeDir = path.join(parent, 'store')
  const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
  const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)
  const info = filesIndex.get('foo.txt')!
  const filePath = getFilePathByModeInCafs(storeDir, info.digest, info.mode)
  expect(fs.statSync(filePath).mode & 0o002).toBe(0)
})

testOnPosix('directories added to a group-writable store get group access under a umask that removes owner read', () => {
  const parent = temporaryDirectory()
  fs.chmodSync(parent, 0o2775)
  const storeDir = path.join(parent, 'store')
  const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
  const previousUmask = process.umask(0o477)
  let filePath: string
  try {
    const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)
    const info = filesIndex.get('foo.txt')!
    filePath = getFilePathByModeInCafs(storeDir, info.digest, info.mode)
  } finally {
    process.umask(previousUmask)
  }
  for (let dir = path.dirname(filePath); dir !== parent; dir = path.dirname(dir)) {
    expect(fs.statSync(dir).mode & 0o2070).toBe(0o2070)
  }
})

test('unpack should not fail when the tarball format seems to be not USTAR or GNU TAR', () => {
  const dest = temporaryDirectory()
  const cafs = createCafs(dest)
  const { filesIndex } = cafs.addFilesFromTarball(
    fs.readFileSync(testFixtures.find('devextreme-17.1.6.tgz'))
  )
  expect(filesIndex.size).toBeGreaterThan(0)
})

test.each([
  { control: '\x1b[31m', escaped: '\\x1B' },
  { control: '\x9b31m', escaped: '\\x9B' },
])('escapes terminal controls in oversized TAR entry diagnostics: $escaped', ({ control, escaped }) => {
  const onFile = jest.fn()
  const parser = createTarballParser(onFile, undefined, 16)
  const header = createTarballWithEntry(`package/bad${control}name`, '', { declaredSize: 17 }).subarray(0, 512)
  let error: unknown
  try {
    parser.push(header)
  } catch (caught) {
    error = caught
  }
  expect(error).toMatchObject({
    code: 'ERR_PNPM_TARBALL_ENTRY_TOO_LARGE',
    message: expect.stringContaining(escaped),
  })
  expect((error as Error).message).not.toContain(control[0])
  expect(onFile).not.toHaveBeenCalled()
})
