/**
 * Script to generate malicious ZIP fixtures for path traversal testing.
 *
 * AdmZip's addFile() sanitizes paths automatically, so we need to create
 * raw ZIP files manually to test path traversal protection.
 *
 * Run with: node --experimental-strip-types scripts/create-fixtures.ts
 */
import fs from 'fs'
import path from 'path'

/**
 * Create a minimal ZIP file with a given entry path (not sanitized).
 * This creates a valid ZIP structure with a single uncompressed file entry.
 */
function createZipWithEntry (entryPath: string, content: string): Buffer {
  const contentBuf = Buffer.from(content)

  // Local file header (30 bytes + filename)
  const localHeader = Buffer.alloc(30 + entryPath.length)
  localHeader.writeUInt32LE(0x04034b50, 0) // Local file header signature
  localHeader.writeUInt16LE(20, 4) // Version needed to extract
  localHeader.writeUInt16LE(0, 6) // General purpose flags
  localHeader.writeUInt16LE(0, 8) // Compression method (0 = store)
  localHeader.writeUInt16LE(0, 10) // Last mod file time
  localHeader.writeUInt16LE(0, 12) // Last mod file date
  localHeader.writeUInt32LE(0, 14) // CRC-32 (fake but okay for tests)
  localHeader.writeUInt32LE(contentBuf.length, 18) // Compressed size
  localHeader.writeUInt32LE(contentBuf.length, 22) // Uncompressed size
  localHeader.writeUInt16LE(entryPath.length, 26) // Filename length
  localHeader.writeUInt16LE(0, 28) // Extra field length
  localHeader.write(entryPath, 30, 'utf-8') // Filename

  const cdOffset = localHeader.length + contentBuf.length

  // Central directory header (46 bytes + filename)
  const centralDir = Buffer.alloc(46 + entryPath.length)
  centralDir.writeUInt32LE(0x02014b50, 0) // Central file header signature
  centralDir.writeUInt16LE(20, 4) // Version made by
  centralDir.writeUInt16LE(20, 6) // Version needed to extract
  centralDir.writeUInt16LE(0, 8) // General purpose flags
  centralDir.writeUInt16LE(0, 10) // Compression method
  centralDir.writeUInt16LE(0, 12) // Last mod file time
  centralDir.writeUInt16LE(0, 14) // Last mod file date
  centralDir.writeUInt32LE(0, 16) // CRC-32
  centralDir.writeUInt32LE(contentBuf.length, 20) // Compressed size
  centralDir.writeUInt32LE(contentBuf.length, 24) // Uncompressed size
  centralDir.writeUInt16LE(entryPath.length, 28) // Filename length
  centralDir.writeUInt16LE(0, 30) // Extra field length
  centralDir.writeUInt16LE(0, 32) // File comment length
  centralDir.writeUInt16LE(0, 34) // Disk number start
  centralDir.writeUInt16LE(0, 36) // Internal file attributes
  centralDir.writeUInt32LE(0, 38) // External file attributes
  centralDir.writeUInt32LE(0, 42) // Relative offset of local header
  centralDir.write(entryPath, 46, 'utf-8')

  // End of central directory record (22 bytes)
  const endRecord = Buffer.alloc(22)
  endRecord.writeUInt32LE(0x06054b50, 0) // End of central directory signature
  endRecord.writeUInt16LE(0, 4) // Number of this disk
  endRecord.writeUInt16LE(0, 6) // Disk with central directory
  endRecord.writeUInt16LE(1, 8) // Entries on this disk
  endRecord.writeUInt16LE(1, 10) // Total entries
  endRecord.writeUInt32LE(centralDir.length, 12) // Size of central directory
  endRecord.writeUInt32LE(cdOffset, 16) // Offset of central directory
  endRecord.writeUInt16LE(0, 20) // ZIP file comment length

  return Buffer.concat([localHeader, contentBuf, centralDir, endRecord])
}

// Ensure fixtures directory exists
const fixturesDir = path.join(import.meta.dirname, '..', 'test', 'fixtures')
fs.mkdirSync(fixturesDir, { recursive: true })

// Create path traversal ZIP (../../../ prefix)
const pathTraversalZip = createZipWithEntry(
  '../../../.npmrc',
  'registry=https://evil.com/\n'
)
fs.writeFileSync(path.join(fixturesDir, 'path-traversal.zip'), pathTraversalZip)
console.log('Created: test/fixtures/path-traversal.zip')

// Create absolute path ZIP (/etc/passwd)
const absolutePathZip = createZipWithEntry(
  '/etc/passwd',
  'root:x:0:0:root:/root:/bin/bash'
)
fs.writeFileSync(path.join(fixturesDir, 'absolute-path.zip'), absolutePathZip)
console.log('Created: test/fixtures/absolute-path.zip')

// Create Windows-style backslash path traversal ZIP
// This is only dangerous on Windows (on Unix, backslash is a valid filename char)
const backslashTraversalZip = createZipWithEntry(
  '..\\..\\..\\evil.txt',
  'malicious content via backslash'
)
fs.writeFileSync(path.join(fixturesDir, 'backslash-traversal.zip'), backslashTraversalZip)
console.log('Created: test/fixtures/backslash-traversal.zip')

console.log('\nDone! Created malicious ZIP fixtures for path traversal testing.')
