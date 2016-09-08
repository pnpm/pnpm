import retry = require('retry')
import isRetryAllowed = require('is-retry-allowed')

export default opts => fn => function () {
  const operation = retry.operation(opts)

  return new Promise((resolve, reject) => {
    operation.attempt(() => {
      fn.apply(null, arguments)
        .then(resolve)
        .catch(err => {
          if (enhancedRetryAllowed(err) && operation.retry(err)) {
            return
          }
          reject(operation.mainError() || err)
        })
    })
  })
}

function enhancedRetryAllowed (err) {
  if (err.statusCode) {
    return isRetryAllowedForStatusCode(err.statusCode)
  }
  return isRetryAllowed(err)
}

function isRetryAllowedForStatusCode (sc) {
  return sc === 408 || sc >= 500
}
