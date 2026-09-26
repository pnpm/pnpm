import path from 'node:path'

export type OnTarballFile = (relativePath: string, mode: number, content: Buffer) => void

export interface TarballParser {
  push: (chunk: Buffer) => void
  end: () => void
}

const ZERO: number = '0'.charCodeAt(0)
const FILE_TYPE_HARD_LINK: number = '1'.charCodeAt(0)
const FILE_TYPE_SYMLINK: number = '2'.charCodeAt(0)
const FILE_TYPE_DIRECTORY: number = '5'.charCodeAt(0)
const SPACE: number = ' '.charCodeAt(0)
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
export function createTarballParser (onFile: OnTarballFile): TarballParser {
  const chunks: Buffer[] = []
  let chunkOffset = 0
  let available = 0
  let consumed = 0
  let finished = false

  let entry: PendingEntry | undefined
  let content: { buffer: Buffer, filled: number } | undefined
  let bytesToSkip = 0

  let longLinkPath = ''
  // If a PAX extended header record is encountered and has a path field, it overrides the next entry's path.
  let paxHeaderPath = ''
  let paxHeaderFileSize: number | undefined

  return { push, end }

  function push (chunk: Buffer): void {
    if (finished || chunk.length === 0) return
    chunks.push(chunk)
    available += chunk.length
    drain()
  }

  function end (): void {
    if (!finished) {
      throw new Error(`Unexpected end of TAR archive at offset ${consumed + available}`)
    }
  }

  function drain (): void {
    while (!finished) {
      if (bytesToSkip > 0) {
        const n = Math.min(bytesToSkip, available)
        discard(n)
        bytesToSkip -= n
        if (bytesToSkip > 0) return
        continue
      }
      if (entry != null) {
        const entryContent = readContent(entry.size)
        if (entryContent == null) return
        handleEntryContent(entry, entryContent)
        bytesToSkip = paddingOf(entry.size)
        entry = undefined
        continue
      }
      if (available === 0) return
      // The archive ends with zero-filled blocks.
      if (chunks[0][chunkOffset] === 0) {
        finished = true
        chunks.length = 0
        available = 0
        return
      }
      if (available < BLOCK_SIZE) return
      const headerOffset = consumed
      const header = take(BLOCK_SIZE)
      const nextEntry = parseHeader(header, headerOffset)
      if (entryHasContent(nextEntry.fileType)) {
        entry = nextEntry
      } else {
        bytesToSkip = nextEntry.size + paddingOf(nextEntry.size)
      }
    }
  }

  /**
   * Returns the next `size` bytes once they have all arrived, copying them
   * out of the pushed chunks as they come so that the chunks can be released.
   */
  function readContent (size: number): Buffer | undefined {
    if (content == null) {
      if (available >= size) return take(size)
      content = { buffer: Buffer.allocUnsafe(size), filled: 0 }
    }
    while (available > 0 && content.filled < size) {
      const chunk = chunks[0]
      const n = Math.min(chunk.length - chunkOffset, size - content.filled)
      chunk.copy(content.buffer, content.filled, chunkOffset, chunkOffset + n)
      content.filled += n
      discard(n)
    }
    if (content.filled < size) return undefined
    const { buffer } = content
    content = undefined
    return buffer
  }

  function take (size: number): Buffer {
    if (size === 0) return Buffer.alloc(0)
    const chunk = chunks[0]
    if (chunk.length - chunkOffset >= size) {
      const view = chunk.subarray(chunkOffset, chunkOffset + size)
      discard(size)
      return view
    }
    const buffer = Buffer.allocUnsafe(size)
    let filled = 0
    while (filled < size) {
      const current = chunks[0]
      const n = Math.min(current.length - chunkOffset, size - filled)
      current.copy(buffer, filled, chunkOffset, chunkOffset + n)
      filled += n
      discard(n)
    }
    return buffer
  }

  function discard (size: number): void {
    chunkOffset += size
    available -= size
    consumed += size
    while (chunks.length > 0 && chunkOffset >= chunks[0].length) {
      chunkOffset -= chunks[0].length
      chunks.shift()
    }
  }

  function handleEntryContent ({ fileType, fileName, mode }: PendingEntry, entryContent: Buffer): void {
    switch (fileType) {
      case FILE_TYPE_PAX_HEADER:
        parsePaxHeader(entryContent, false)
        break
      case FILE_TYPE_PAX_GLOBAL_HEADER:
        parsePaxHeader(entryContent, true)
        break
      case FILE_TYPE_LONGLINK: {
        longLinkPath = entryContent.toString('utf8').replace(/\0.*/, '')
        // Remove the first path segment
        const slashIndex = longLinkPath.indexOf('/')
        if (slashIndex >= 0) {
          longLinkPath = longLinkPath.slice(slashIndex + 1)
        }
        break
      }
      default:
        onFile(fileName, mode, entryContent)
    }
  }

  function parseHeader (header: Buffer, headerOffset: number): PendingEntry {
    // The file type is a single byte at offset 156 in the header
    const fileType = header[FILE_TYPE_OFFSET]
    let fileSize: number
    if (paxHeaderFileSize !== undefined) {
      fileSize = paxHeaderFileSize
      paxHeaderFileSize = undefined
    } else {
      // The file size is an octal number encoded as UTF-8. It is terminated by a NUL or space. Maximum length 12 characters.
      fileSize = parseOctal(header, FILE_SIZE_OFFSET, 12)
    }

    const expectedCheckSum: number = parseOctal(header, CHECKSUM_OFFSET, 8)
    const actualCheckSum: number = checkSum(header)
    if (expectedCheckSum !== actualCheckSum) {
      throw new Error(
        `Invalid checksum for TAR header at offset ${headerOffset}. Expected ${expectedCheckSum}, got ${actualCheckSum}`
      )
    }

    let fileName: string
    if (longLinkPath) {
      fileName = longLinkPath
      longLinkPath = ''
    } else if (paxHeaderPath) {
      fileName = paxHeaderPath

      // The PAX header only applies to the immediate next entry.
      paxHeaderPath = ''
    } else {
      fileName = parseHeaderPath(header)
    }

    if (fileName.includes('./') || fileName.includes('.\\')) {
      // Normalize path traversal attempts (including Windows backslash traversal)
      // Replaces backslashes with forward slashes and uses POSIX path normalization to resolve ..
      fileName = path.posix.join('/', fileName.replaceAll('\\', '/')).slice(1)
    }

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

  /**
   * Parses a PAX header, which is a series of key/value pairs.
   *
   * @param buffer - The content of the PAX header entry
   * @param global - Whether this is a global PAX header
   */
  function parsePaxHeader (buffer: Buffer, global: boolean): void {
    const end: number = buffer.length
    let i: number = 0
    while (i < end) {
      const lineStart: number = i
      while (i < end && buffer[i] !== SPACE) {
        i++
      }

      // The format of a PAX header line is "%d %s=%s\n"
      const strLen: string = buffer.toString('utf-8', lineStart, i)
      const len: number = parseInt(strLen, 10)
      if (!len) {
        throw new Error(`Invalid length in PAX record: ${strLen}`)
      }

      // Skip the space.
      i++

      const lineEnd: number = lineStart + len

      const record: string = buffer.toString('utf-8', i, lineEnd - 1)
      i = lineEnd

      const equalSign: number = record.indexOf('=')
      const keyword: string = record.slice(0, equalSign)

      if (keyword === 'path') {
        // Still need to trim the first path segment.
        const slashIndex: number = record.indexOf('/', equalSign + 1)
        if (global) {
          throw new Error(`Unexpected global PAX path: ${record}`)
        }
        paxHeaderPath = record.slice(slashIndex >= 0 ? slashIndex + 1 : equalSign + 1)
      } else if (keyword === 'size') {
        const size: number = parseInt(record.slice(equalSign + 1), 10)
        if (isNaN(size) || size < 0) {
          throw new Error(`Invalid size in PAX record: ${record}`)
        }
        if (global) {
          throw new Error(`Unexpected global PAX file size: ${record}`)
        }
        paxHeaderFileSize = size
      }
    }
  }
}

function entryHasContent (fileType: number): boolean {
  return fileType !== FILE_TYPE_DIRECTORY && fileType !== FILE_TYPE_SYMLINK
}

/**
 * An entry's content is followed by zeros up to the next 512-byte block boundary.
 */
function paddingOf (size: number): number {
  return (BLOCK_SIZE - (size & 0x1ff)) & 0x1ff
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
  let i: number = 0

  for (; i < CHECKSUM_OFFSET; i++) {
    sum += header[i]
  }

  for (i = FILE_TYPE_OFFSET; i < BLOCK_SIZE; i++) {
    sum += header[i]
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
