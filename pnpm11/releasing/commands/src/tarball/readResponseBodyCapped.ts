/**
 * Read a response into memory up to `maxBytes`. Returns `undefined` and
 * cancels the stream as soon as the response exceeds the limit.
 */
export async function readResponseBodyCapped (response: Response, maxBytes: number): Promise<Buffer | undefined> {
  const reader = response.body?.getReader()
  if (reader == null) return Buffer.alloc(0)
  const chunks: Buffer[] = []
  let total = 0
  for (;;) {
    // eslint-disable-next-line no-await-in-loop
    const { done, value } = await reader.read()
    if (done) break
    total += value.byteLength
    if (total > maxBytes) {
      // eslint-disable-next-line no-await-in-loop
      await reader.cancel().catch(() => {})
      return undefined
    }
    chunks.push(Buffer.from(value))
  }
  return Buffer.concat(chunks)
}
