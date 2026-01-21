import * as fs from 'fs'
import { Packr } from 'msgpackr'

/**
 * Create a Packr instance with record structure optimization enabled.
 * This provides 2-3x faster decoding performance by reusing object structures
 * and using integer keys instead of string property names.
 */
const packr = new Packr({
  useRecords: true,
  // moreTypes: true enables better type preservation for undefined, etc.
  moreTypes: true,
})

/**
 * Write data to a file in msgpack format (synchronous)
 */
export function writeFileSync (filePath: string, data: unknown): void {
  const buffer = packr.pack(data)
  fs.writeFileSync(filePath, buffer)
}

/**
 * Read msgpack data from a file (synchronous)
 */
export function readFileSync<T> (filePath: string): T {
  const buffer = fs.readFileSync(filePath)
  return packr.unpack(buffer) as T
}

/**
 * Read msgpack data from a file (async)
 */
export async function readFile<T> (filePath: string): Promise<T> {
  const buffer = await fs.promises.readFile(filePath)
  return packr.unpack(buffer) as T
}

/**
 * Write data to a file in msgpack format (async)
 */
export async function writeFile (filePath: string, data: unknown): Promise<void> {
  const buffer = packr.pack(data)
  await fs.promises.writeFile(filePath, buffer)
}
