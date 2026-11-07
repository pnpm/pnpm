import { createReadStream, createWriteStream } from 'node:fs'
import { pipeline } from 'node:stream/promises'
import { createGunzip } from 'node:zlib'

const BLOCK_SIZE = 512
/** Regular file, in both the old (`\0`) and the ustar (`0`) spelling. */
const FILE_TYPES = new Set(['0', '\0'])

/**
 * Writes one file out of a package tarball to `dest`, and nothing else.
 *
 * Reads the archive in-process rather than shelling out to `tar` the way
 * {@link extractTarball} does: a caller that wants a single executable, in an
 * environment it does not control (a Corepack cache in a minimal image), should
 * not need a `tar` on the PATH for it. Only regular files are considered, which
 * is all an npm tarball holds beyond directories, and the parse stays streaming
 * so the archive never lands in memory.
 *
 * `dest` is created exclusively — an existing file, or a symlink planted at
 * that path, fails the write rather than being followed.
 *
 * @param tarball Path to the gzipped archive.
 * @param memberPath Path of the wanted file inside it, e.g. `package/pnpm`.
 * @param dest Path to write it to.
 * @param mode Permissions for `dest`.
 * @returns whether the member was there.
 */
export async function extractTarballMember (
  tarball: string,
  memberPath: string,
  dest: string,
  mode = 0o644
): Promise<boolean> {
  let found = false
  await pipeline(
    createReadStream(tarball),
    createGunzip(),
    async function (source: AsyncIterable<Buffer>): Promise<void> {
      const reader = new BlockReader(source)
      const entry = await seekMember(reader, memberPath)
      if (entry == null) return
      const written = await reader.pipe(entry.size, createWriteStream(dest, { flags: 'wx', mode }))
      if (written !== entry.size) {
        throw new Error(`${tarball} ends after ${written} of the ${entry.size} bytes it declares for ${memberPath}.`)
      }
      found = true
      // The rest of the archive holds nothing this caller asked for, and the
      // checksum that vouches for it was checked before any of it was read.
    }
  )
  return found
}

/**
 * Reads up to the content of the regular file at `memberPath`.
 *
 * @returns its header, or `null` when the archive has no such file.
 */
async function seekMember (reader: BlockReader, memberPath: string): Promise<TarEntry | null> {
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- a tar archive can only be walked one entry after another
    const header = await reader.read(BLOCK_SIZE)
    // The archive ends with zero-filled blocks; one is enough to stop.
    if (header == null || header[0] === 0) return null
    const entry = parseHeader(header)
    if (FILE_TYPES.has(entry.type) && entry.path === memberPath) return entry
    // eslint-disable-next-line no-await-in-loop -- a tar archive can only be walked one entry after another
    await reader.skip(Math.ceil(entry.size / BLOCK_SIZE) * BLOCK_SIZE)
  }
}

interface TarEntry {
  path: string
  size: number
  type: string
}

function parseHeader (header: Buffer): TarEntry {
  const name = readString(header, 0, 100)
  const prefix = readString(header, 345, 155)
  return {
    path: prefix === '' ? name : `${prefix}/${name}`,
    size: parseInt(readString(header, 124, 12).trim() || '0', 8),
    type: String.fromCharCode(header[156]!),
  }
}

function readString (header: Buffer, start: number, length: number): string {
  const field = header.subarray(start, start + length)
  const end = field.indexOf(0)
  return field.toString('utf8', 0, end === -1 ? field.length : end)
}

class BlockReader {
  readonly #iterator: AsyncIterator<Buffer>
  #buffered: Buffer[] = []
  #buffedBytes = 0
  #done = false

  constructor (source: AsyncIterable<Buffer>) {
    this.#iterator = source[Symbol.asyncIterator]()
  }

  /** The next `size` bytes, or `null` once the stream ends. */
  async read (size: number): Promise<Buffer | null> {
    if (!await this.#fill(size)) return null
    return this.#take(size)
  }

  async skip (size: number): Promise<void> {
    for await (const _chunk of this.#chunks({ left: size })) {
      // Discarded.
    }
  }

  /**
   * Hands the next `size` bytes to `destination`, without collecting them.
   *
   * @returns how many bytes there were, which is fewer than `size` only when
   * the stream ended early.
   */
  async pipe (size: number, destination: NodeJS.WritableStream): Promise<number> {
    const progress = { left: size }
    await pipeline(this.#chunks(progress), destination)
    return size - progress.left
  }

  /** The next `progress.left` bytes, counting them down as they are yielded. */
  async * #chunks (progress: { left: number }): AsyncGenerator<Buffer> {
    while (progress.left > 0) {
      // eslint-disable-next-line no-await-in-loop -- the stream is read in order, one chunk at a time
      if (!await this.#fill(1)) return
      const chunk = this.#take(Math.min(progress.left, this.#buffedBytes))
      progress.left -= chunk.length
      yield chunk
    }
  }

  /** Reads until at least `size` bytes are buffered, or the stream ends. */
  async #fill (size: number): Promise<boolean> {
    while (this.#buffedBytes < size && !this.#done) {
      // eslint-disable-next-line no-await-in-loop -- the stream is read in order, one chunk at a time
      const { value, done } = await this.#iterator.next()
      if (done === true) {
        this.#done = true
      } else {
        this.#buffered.push(value)
        this.#buffedBytes += value.length
      }
    }
    return this.#buffedBytes >= size
  }

  #take (size: number): Buffer {
    const joined = this.#buffered.length === 1 ? this.#buffered[0]! : Buffer.concat(this.#buffered)
    this.#buffered = joined.length > size ? [joined.subarray(size)] : []
    this.#buffedBytes = joined.length - size
    return joined.subarray(0, size)
  }
}
