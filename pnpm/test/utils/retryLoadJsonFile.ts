import loadJsonFile from 'load-json-file'
import * as retry from '@zkochan/retry'

export function retryLoadJsonFile<T> (filePath: string): Promise<T> {
  const operation = retry.operation({})

  return new Promise<T>((resolve, reject) => {
    operation.attempt(async (currentAttempt) => {
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

export function retryLoadJsonFile2<T> (filePath: string): { value: Promise<T>, abortController: () => void } {
  const operation = retry.operation({})

  return {
    value: new Promise<T>((resolve, reject) => {
      operation.attempt(async (currentAttempt) => {
        try {
          resolve(await loadJsonFile<T>(filePath))
        } catch (err: any) { // eslint-disable-line
          if (operation.retry(err)) {
            return
          }
          reject(err)
        }
      })
    }),
    abortController: () => {
      operation.stop()
    },
  }
}
