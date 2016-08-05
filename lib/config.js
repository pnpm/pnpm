module.exports = require('rc')('pnpm', {
  concurrency: 16,
  storePath: 'node_modules/.store',
  logger: 'pretty'
})
