import loadJsonFile = require('load-json-file')
import retry = require('retry')

export default (filePath: string): Promise<any> => {
  const operation = retry.operation()

  return new Promise((resolve, reject) => {
    operation.attempt(async (currentAttempt) => {
      try {
        resolve(await loadJsonFile(filePath))
      } catch (err) {
        if (err.code === 'ENOENT' && operation.retry(err)) {
          return
        }
        reject(err)
      }
    })
  })
}
