import { open, readFile } from 'node:fs/promises'

import { temporaryFileTask } from 'tempy'

/**
 * Read a response into memory up to `maxBytes` without retaining a second
 * in-memory copy. A missing body returns an empty buffer. A response over the
 * limit returns `undefined` after a best-effort stream cancellation. Stream
 * and temporary-file errors are propagated.
 */
export async function readResponseBodyCapped (response: Response, maxBytes: number): Promise<Buffer | undefined> {
  const reader = response.body?.getReader()
  if (reader == null) return Buffer.alloc(0)

  return temporaryFileTask(async (temporaryPath) => {
    const file = await open(temporaryPath, 'wx', 0o600)
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
