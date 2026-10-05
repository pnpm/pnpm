import path from 'node:path'

import { assertBufferedTarballEntry } from './assertBufferedTarballEntry.js'

export type OnTarballFile = (relativePath: string, mode: number, content: Buffer) => void

export interface TarballFileWriter {
  write: (chunk: Buffer) => void
  end: () => void
}

export type CreateTarballFileWriter = (relativePath: string, mode: number, size: number) => TarballFileWriter | undefined

export interface TarballParser {
  push: (chunk: Buffer) => void
  end: () => void
}

const ZERO: number = '0'.charCodeAt(0)
const FILE_TYPE_HARD_LINK: number = '1'.charCodeAt(0)
const FILE_TYPE_SYMLINK: number = '2'.charCodeAt(0)
const FILE_TYPE_DIRECTORY: number = '5'.charCodeAt(0)
const SPACE: number = ' '.charCodeAt(0)
const NEWLINE: number = '\n'.charCodeAt(0)
const SLASH: number = '/'.charCodeAt(0)
const BACKSLASH: number = '\\'.charCodeAt(0)
const FILE_TYPE_PAX_HEADER: number = 'x'.charCodeAt(0)
const FILE_TYPE_PAX_GLOBAL_HEADER: number = 'g'.charCodeAt(0)
const FILE_TYPE_LONGLINK: number = 'L'.charCodeAt(0)

const BLOCK_SIZE = 512
const MODE_OFFSET = 100
const FILE_SIZE_OFFSET = 124
const CHECKSUM_OFFSET = 148
const FILE_TYPE_OFFSET = 156
const PREFIX_OFFSET = 345

interface PendingEntry {
  fileType: number
  fileName: string
  mode: number
  size: number
}

/**
 * Parses a TAR archive fed to it in chunks of any size, calling `onFile` for
 * every regular file (and hard link) in archive order. A later entry with the
 * same path supersedes an earlier one, so callers must let the last call win.
 *
 * An entry's content is handed out as a zero-copy view when it lies within a
 * single pushed chunk, so pushing a whole archive at once allocates nothing
 * per file. Otherwise the parser holds at most one entry's content at a time.
 *
 * See the TAR specification: https://www.gnu.org/software/tar/manual/html_node/Standard.html
 */
export function createTarballParser (onFile: OnTarballFile, createFileWriter?: CreateTarballFileWriter, maxBufferedEntrySize?: number): TarballParser {
  const state: ParserState = {
    onFile,
    createFileWriter,
    maxBufferedEntrySize,
    writer: undefined,
    chunks: [],
    chunkOffset: 0,
    available: 0,
    consumed: 0,
    finished: false,
    entry: undefined,
    content: undefined,
    bytesToSkip: 0,
    longLinkPath: '',
    paxHeaderPath: '',
    paxHeaderFileSize: undefined,
  }
  return {
    push: (chunk) => pushChunk(state, chunk),
    end: () => assertArchiveFinished(state),
  }
}

interface ParserState {
  onFile: OnTarballFile
  createFileWriter?: CreateTarballFileWriter
  maxBufferedEntrySize?: number
  writer: { sink: TarballFileWriter, remaining: number } | undefined
  chunks: Buffer[]
  chunkOffset: number
  available: number
  consumed: number
  finished: boolean
  entry: PendingEntry | undefined
  content: { parts: Buffer[], received: number } | undefined
  bytesToSkip: number
  longLinkPath: string
  // If a PAX extended header record is encountered and has a path field, it overrides the next entry's path.
  paxHeaderPath: string
  paxHeaderFileSize: number | undefined
}

function pushChunk (state: ParserState, chunk: Buffer): void {
  if (state.finished || chunk.length === 0) return
  state.chunks.push(chunk)
  state.available += chunk.length
  drain(state)
}

function assertArchiveFinished (state: ParserState): void {
  if (!state.finished) {
    throw new Error(`Unexpected end of TAR archive at offset ${state.consumed + state.available}`)
  }
}

function drain (state: ParserState): void {
  while (!state.finished) {
    if (!drainStep(state)) return
  }
}

/**
 * Makes one step of progress through the buffered bytes.
 * Returns `false` when the parser has to wait for more input or the archive has ended.
 */
function drainStep (state: ParserState): boolean {
  if (state.bytesToSkip > 0) return skipBytes(state)
  if (state.entry != null) return consumeEntryContent(state, state.entry)
  return consumeHeader(state)
}

function skipBytes (state: ParserState): boolean {
  const skipped = Math.min(state.bytesToSkip, state.available)
  discard(state, skipped)
  state.bytesToSkip -= skipped
  return state.bytesToSkip === 0
}

function consumeEntryContent (state: ParserState, entry: PendingEntry): boolean {
  if (state.writer) {
    if (!consumeStreamedContent(state)) return false
  } else {
    const entryContent = readContent(state, entry.size)
    if (entryContent == null) return false
    handleEntryContent(state, entry, entryContent)
  }
  state.bytesToSkip = paddingOf(entry.size)
  state.entry = undefined
  return true
}

function consumeStreamedContent (state: ParserState): boolean {
  const writer = state.writer!
  while (state.available > 0 && writer.remaining > 0) {
    const chunk = state.chunks[0]
    const size = Math.min(chunk.length - state.chunkOffset, writer.remaining)
    writer.sink.write(chunk.subarray(state.chunkOffset, state.chunkOffset + size))
    discard(state, size)
    writer.remaining -= size
  }
  if (writer.remaining > 0) return false
  writer.sink.end()
  state.writer = undefined
  return true
}

function startEntryWriter (state: ParserState, entry: PendingEntry): void {
  if (entry.fileType !== 0 && entry.fileType !== ZERO && entry.fileType !== FILE_TYPE_HARD_LINK) return
  const sink = state.createFileWriter?.(entry.fileName, entry.mode, entry.size)
  if (sink) state.writer = { sink, remaining: entry.size }
}

function consumeHeader (state: ParserState): boolean {
  if (state.available === 0) return false
  // The archive ends with zero-filled blocks.
  if (state.chunks[0][state.chunkOffset] === 0) {
    state.finished = true
    state.chunks.length = 0
    state.available = 0
    return false
  }
  if (state.available < BLOCK_SIZE) return false
  const headerOffset = state.consumed
  const header = take(state, BLOCK_SIZE)
  const nextEntry = parseHeader(state, header, headerOffset)
  if (entryHasContent(nextEntry.fileType)) {
    state.entry = nextEntry
    startEntryWriter(state, nextEntry)
    if (!state.writer) assertBufferedTarballEntry(nextEntry, state.maxBufferedEntrySize)
  } else {
    state.bytesToSkip = nextEntry.size + paddingOf(nextEntry.size)
  }
  return true
}

/**
 * Returns the next `size` bytes once they have all arrived. Until then, the
 * parser holds only the bytes received, so a header that declares more
 * content than the archive has does not reserve memory for it.
 */
function readContent (state: ParserState, size: number): Buffer | undefined {
  if (state.content == null) {
    if (state.available >= size) return take(state, size)
    state.content = { parts: [], received: 0 }
  }
  const { content } = state
  while (state.available > 0 && content.received < size) {
    const chunk = state.chunks[0]
    const length = Math.min(chunk.length - state.chunkOffset, size - content.received)
    content.parts.push(chunk.subarray(state.chunkOffset, state.chunkOffset + length))
    content.received += length
    discard(state, length)
  }
  if (content.received < size) return undefined
  state.content = undefined
  return Buffer.concat(content.parts, size)
}

function take (state: ParserState, size: number): Buffer {
  if (size === 0) return Buffer.alloc(0)
  const chunk = state.chunks[0]
  if (chunk.length - state.chunkOffset >= size) {
    const view = chunk.subarray(state.chunkOffset, state.chunkOffset + size)
    discard(state, size)
    return view
  }
  const buffer = Buffer.allocUnsafe(size)
  let filled = 0
  while (filled < size) {
    const current = state.chunks[0]
    const length = Math.min(current.length - state.chunkOffset, size - filled)
    current.copy(buffer, filled, state.chunkOffset, state.chunkOffset + length)
    filled += length
    discard(state, length)
  }
  return buffer
}

function discard (state: ParserState, size: number): void {
  state.chunkOffset += size
  state.available -= size
  state.consumed += size
  while (state.chunks.length > 0 && state.chunkOffset >= state.chunks[0].length) {
    state.chunkOffset -= state.chunks[0].length
    state.chunks.shift()
  }
}

function handleEntryContent (state: ParserState, { fileType, fileName, mode }: PendingEntry, entryContent: Buffer): void {
  switch (fileType) {
    case FILE_TYPE_PAX_HEADER:
      parsePaxHeader(state, entryContent, false)
      break
    case FILE_TYPE_PAX_GLOBAL_HEADER:
      parsePaxHeader(state, entryContent, true)
      break
    case FILE_TYPE_LONGLINK:
      state.longLinkPath = parseLongLinkPath(entryContent)
      break
    default:
      state.onFile(fileName, mode, entryContent)
  }
}

function parseLongLinkPath (entryContent: Buffer): string {
  const longLinkPath = entryContent.toString('utf8').replace(/\0.*/, '')
  // Remove the first path segment
  const slashIndex = longLinkPath.indexOf('/')
  return slashIndex >= 0 ? longLinkPath.slice(slashIndex + 1) : longLinkPath
}

function parseHeader (state: ParserState, header: Buffer, headerOffset: number): PendingEntry {
  const fileType = header[FILE_TYPE_OFFSET]
  const fileSize = takeEntryFileSize(state, header)
  verifyHeaderChecksum(header, headerOffset)
  if (!Number.isSafeInteger(fileSize) || fileSize < 0) {
    throw new Error(`Invalid file size for TAR header at offset ${headerOffset}`)
  }
  const fileName = normalizeTraversal(takeEntryFileName(state, header))

  // Values '\0' and '0' are normal files.
  // Treat all other file types as non-existent
  // However, we still need to parse the name to handle collisions
  switch (fileType) {
    case 0:
    case ZERO:
    case FILE_TYPE_HARD_LINK:
      return {
        fileType,
        fileName: fileName.replaceAll('//', '/'),
        // The file mode is an octal number encoded as UTF-8. It is terminated by a NUL or space. Maximum length 8 characters.
        mode: parseOctal(header, MODE_OFFSET, 8),
        size: fileSize,
      }
    case FILE_TYPE_DIRECTORY:
    case FILE_TYPE_SYMLINK:
    case FILE_TYPE_PAX_HEADER:
    case FILE_TYPE_PAX_GLOBAL_HEADER:
    case FILE_TYPE_LONGLINK:
      return { fileType, fileName, mode: 0, size: fileSize }
    default:
      throw new Error(`Unsupported file type ${fileType} for file ${fileName}.`)
  }
}

function takeEntryFileSize (state: ParserState, header: Buffer): number {
  if (state.paxHeaderFileSize !== undefined) {
    const fileSize = state.paxHeaderFileSize
    state.paxHeaderFileSize = undefined
    return fileSize
  }
  // The file size is an octal number encoded as UTF-8. It is terminated by a NUL or space. Maximum length 12 characters.
  return parseOctal(header, FILE_SIZE_OFFSET, 12)
}

function verifyHeaderChecksum (header: Buffer, headerOffset: number): void {
  const expectedCheckSum: number = parseOctal(header, CHECKSUM_OFFSET, 8)
  const actualCheckSum: number = checkSum(header)
  if (expectedCheckSum !== actualCheckSum) {
    throw new Error(
      `Invalid checksum for TAR header at offset ${headerOffset}. Expected ${expectedCheckSum}, got ${actualCheckSum}`
    )
  }
}

function takeEntryFileName (state: ParserState, header: Buffer): string {
  if (state.longLinkPath) {
    const fileName = state.longLinkPath
    state.longLinkPath = ''
    return fileName
  }
  if (state.paxHeaderPath) {
    const fileName = state.paxHeaderPath
    // The PAX header only applies to the immediate next entry.
    state.paxHeaderPath = ''
    return fileName
  }
  return parseHeaderPath(header)
}

function normalizeTraversal (fileName: string): string {
  if (fileName.includes('./') || fileName.includes('.\\')) {
    // Normalize path traversal attempts (including Windows backslash traversal)
    // Replaces backslashes with forward slashes and uses POSIX path normalization to resolve ..
    return path.posix.join('/', fileName.replaceAll('\\', '/')).slice(1)
  }
  return fileName
}

/**
 * Parses a PAX header, which is a series of key/value pairs.
 */
function parsePaxHeader (state: ParserState, buffer: Buffer, global: boolean): void {
  let cursor: number = 0
  while (cursor < buffer.length) {
    const { record, lineEnd } = readPaxRecord(buffer, cursor)
    cursor = lineEnd
    applyPaxRecord(state, record, global)
  }
}

function readPaxRecord (buffer: Buffer, lineStart: number): { record: string, lineEnd: number } {
  const end: number = buffer.length
  let cursor: number = lineStart
  while (cursor < end && buffer[cursor] !== SPACE) {
    cursor++
  }
  if (cursor >= end) {
    throw new Error('Invalid PAX record format: missing space delimiter')
  }

  // The format of a PAX header line is "%d %s=%s\n"
  const strLen: string = buffer.toString('utf-8', lineStart, cursor)
  const len: number = parseInt(strLen, 10)
  if (isNaN(len) || len <= 0 || lineStart + len > end || lineStart + len <= cursor) {
    throw new Error(`Invalid length in PAX record: ${strLen}`)
  }

  // Skip the space.
  cursor++

  const lineEnd: number = lineStart + len
  if (buffer[lineEnd - 1] !== NEWLINE) {
    throw new Error('Invalid PAX record format: missing newline terminator')
  }
  return { record: buffer.toString('utf-8', cursor, lineEnd - 1), lineEnd }
}

function applyPaxRecord (state: ParserState, record: string, global: boolean): void {
  const equalSign: number = record.indexOf('=')
  const keyword: string = record.slice(0, equalSign)

  if (keyword === 'path') {
    // Still need to trim the first path segment.
    const slashIndex: number = record.indexOf('/', equalSign + 1)
    if (global) {
      throw new Error(`Unexpected global PAX path: ${record}`)
    }
    state.paxHeaderPath = record.slice(slashIndex >= 0 ? slashIndex + 1 : equalSign + 1)
  } else if (keyword === 'size') {
    state.paxHeaderFileSize = parsePaxSize(record, equalSign, global)
  }
}

function parsePaxSize (record: string, equalSign: number, global: boolean): number {
  const size: number = parseInt(record.slice(equalSign + 1), 10)
  if (isNaN(size) || size < 0 || !Number.isSafeInteger(size)) {
    throw new Error(`Invalid size in PAX record: ${record}`)
  }
  if (global) {
    throw new Error(`Unexpected global PAX file size: ${record}`)
  }
  return size
}

function entryHasContent (fileType: number): boolean {
  return fileType !== FILE_TYPE_DIRECTORY && fileType !== FILE_TYPE_SYMLINK
}

/**
 * An entry's content is followed by zeros up to the next 512-byte block boundary.
 */
export function paddingOf (size: number): number {
  return (BLOCK_SIZE - (size % BLOCK_SIZE)) % BLOCK_SIZE
}

/**
 * The full file path is an optional prefix at offset 345, followed by the file name at offset 0, separated by a '/'.
 * The first path segment of the full path is dropped.
 */
function parseHeaderPath (header: Buffer): string {
  const trimState = { pathTrimmed: false }
  // Both values are terminated by a NUL if not using the full length of the field.
  let prefix = parseString(header, PREFIX_OFFSET, 155, trimState)

  // If the prefix is present and did not contain a `/` or `\\`, then the prefix is the first path segment and should be dropped entirely.
  if (prefix && !trimState.pathTrimmed) {
    trimState.pathTrimmed = true
    prefix = ''
  }

  // Get the base filename at offset 0, up to 100 characters (where the mode field begins).
  const fileName = parseString(header, 0, MODE_OFFSET, trimState)

  // If the prefix was not trimmed entirely (or absent), need to join with the remaining filename
  return prefix ? `${prefix}/${fileName}` : fileName
}

/**
 * Parses a UTF-8 string at the specified `offset`, up to `length` characters. If it ends early, it will be terminated by a NUL.
 * Will trim the first segment if `pathTrimmed` is currently false and the string contains a `/` or `\\`.
 */
function parseString (buffer: Buffer, offset: number, length: number, trimState: { pathTrimmed: boolean }): string {
  let end: number = offset
  const max: number = length + offset
  for (let char: number = buffer[end]; char !== 0 && end !== max; char = buffer[++end]) {
    if (!trimState.pathTrimmed && (char === SLASH || char === BACKSLASH)) {
      trimState.pathTrimmed = true
      offset = end + 1
    }
  }
  return buffer.toString('utf8', offset, end)
}

/**
 * Computes the checksum of a TAR header, counting the checksum field itself as spaces.
 */
function checkSum (header: Buffer): number {
  let sum: number = 256
  let offset: number = 0

  for (; offset < CHECKSUM_OFFSET; offset++) {
    sum += header[offset]
  }

  for (offset = FILE_TYPE_OFFSET; offset < BLOCK_SIZE; offset++) {
    sum += header[offset]
  }

  return sum
}

/**
 * Parses an octal number at the specified `offset`, up to `length` characters. If it ends early, it will be terminated by either
 * a NUL or a space.
 */
function parseOctal (buffer: Buffer, offset: number, length: number): number {
  const val = buffer.subarray(offset, offset + length)
  offset = 0

  // Older versions of tar can prefix with spaces
  while (offset < val.length && val[offset] === SPACE) offset++
  const end = clamp(indexOf(val, SPACE, offset, val.length), val.length, val.length)
  while (offset < end && val[offset] === 0) offset++
  if (end === offset) return 0
  return parseInt(val.subarray(offset, end).toString(), 8)
}

function indexOf (block: Buffer, num: number, offset: number, end: number): number {
  for (; offset < end; offset++) {
    if (block[offset] === num) return offset
  }
  return end
}

function clamp (index: number, len: number, defaultValue: number): number {
  if (typeof index !== 'number') return defaultValue
  index = ~~index // Coerce to integer.
  if (index >= len) return len
  if (index >= 0) return index
  index += len
  if (index >= 0) return index
  return 0
}
