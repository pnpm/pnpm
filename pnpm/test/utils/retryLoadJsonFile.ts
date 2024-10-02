import loadJsonFile from 'load-json-file'
import * as retry from '@zkochan/retry'

export async function retryLoadJsonFile<T> (filePath: string): Promise<T> {
  const operation = retry.operation({})

  return new Promise<T>((resolve, reject) => {
    operation.attempt(async () => {
      try {
        resolve(await loadJsonFile<T>(filePath))
      } catch (err: any) { // eslint-disable-line
        if (operation.retry(err)) {
          return
        }
        reject(err)
      }
    })
  })
}
