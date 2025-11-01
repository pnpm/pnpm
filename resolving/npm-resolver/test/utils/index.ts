import { safeReadV8FileSync } from '@pnpm/fs.v8-file'

export async function retryLoadJsonFile<T> (filePath: string): Promise<T> {
  let retry = 0
  /* eslint-disable no-await-in-loop */
  while (true) {
    await delay(500)
    try {
      const result = safeReadV8FileSync<T>(filePath)
      if (!result) {
        throw new Error('not found')
      }
      return result
    } catch (err: any) { // eslint-disable-line
      if (retry > 2) throw err
      retry++
    }
  }
  /* eslint-enable no-await-in-loop */
}

export async function delay (time: number): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(() => {
    resolve()
  }, time))
}
