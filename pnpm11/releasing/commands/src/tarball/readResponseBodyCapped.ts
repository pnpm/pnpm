import { open, readFile } from 'node:fs/promises'

import { temporaryFileTask } from 'tempy'

/**
 * Read a response into memory up to `maxBytes` without retaining a second
 * in-memory copy. Returns `undefined` and cancels the stream as soon as the
 * response exceeds the limit.
 */
export async function readResponseBodyCapped (response: Response, maxBytes: number): Promise<Buffer | undefined> {
  const reader = response.body?.getReader()
  if (reader == null) return Buffer.alloc(0)

  return temporaryFileTask(async (temporaryPath) => {
    const file = await open(temporaryPath, 'w')
    let total = 0
    try {
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
        // eslint-disable-next-line no-await-in-loop
        await file.writeFile(value)
      }
    } finally {
      await file.close()
    }
    return readFile(temporaryPath)
  })
}
