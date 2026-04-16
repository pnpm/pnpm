/**
 * Parse an uncompressed /v1/files payload and write each file to the CAFS.
 * Returns the number of files newly written (EEXIST is not counted).
 */
export function writeFiles (storeDir: string, payload: Buffer): number

/**
 * Streaming writer: push chunks as they arrive off the wire; the parser
 * dispatches each complete file to a thread pool so writes overlap with
 * the rest of the download. Call `finish` once to block until all
 * in-flight writes are done and receive the total count.
 */
export class CafsStreamWriter {
  constructor (storeDir: string)
  push (chunk: Buffer): void
  finish (): number
}
