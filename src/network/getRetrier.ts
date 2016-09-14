import retry = require('retry')
import isRetryAllowed = require('is-retry-allowed')

export type RetrierOptions = {
  retries: number,
  factor: number,
  minTimeout: number,
  maxTimeout: number
}

export default (opts: RetrierOptions) => (fn: Function) => function () {
  const operation = retry.operation(opts)

  return new Promise((resolve, reject) => {
    operation.attempt(() => {
      fn.apply(null, arguments)
        .then(resolve)
        .catch((err: Error) => {
          if (enhancedRetryAllowed(err) && operation.retry(err)) {
            return
          }
          reject(operation.mainError() || err)
        })
    })
  })
}

function enhancedRetryAllowed (err: Error) {
  if (err['statusCode']) {
    return isRetryAllowedForStatusCode(err['statusCode'])
  }
  return isRetryAllowed(err)
}

function isRetryAllowedForStatusCode (sc: number) {
  return sc === 408 || sc >= 500
}
