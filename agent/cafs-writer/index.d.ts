/**
 * Parse an uncompressed /v1/files payload and write each file to the CAFS.
 * Returns the number of files newly written (EEXIST is not counted).
 */
export function writeFiles (storeDir: string, payload: Buffer): number

/**
 * POST the given digest list to `{agentUrl}/v1/files`, gunzip the response,
 * parse it, and write each file to the CAFS in parallel. The HTTP round
 * trip, gzip decode, and disk writes all happen inside Rust — the JS side
 * only batches and invokes this once per batch.
 */
export function fetchBatch (
  agentUrl: string,
  digests: Array<{ digest: string, size: number, executable: boolean }>,
  storeDir: string
): number

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
