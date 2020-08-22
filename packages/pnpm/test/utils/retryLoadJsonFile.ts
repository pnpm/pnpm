import retry = require('@zkochan/retry')
import loadJsonFile = require('load-json-file')

export default <T>(filePath: string): Promise<T> => {
  const operation = retry.operation({})

  return new Promise<T>((resolve, reject) => {
    // eslint-disable-next-line @typescript-eslint/no-misused-promises
    operation.attempt(async (currentAttempt) => {
      try {
        resolve(await loadJsonFile<T>(filePath))
      } catch (err) {
        if (operation.retry(err)) {
          return
        }
        reject(err)
      }
    })
  })
}
