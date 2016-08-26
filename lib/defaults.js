'use strict'
module.exports = {
  concurrency: 16,
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4, // 10 seconds
  fetchRetryMaxtimeout: 6e4, // 1 minute
  storePath: 'node_modules/.store',
  logger: 'pretty'
}
