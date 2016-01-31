module.exports = require('rc')('pnpm', {
  concurrency: 16,
  store_path: 'node_modules/.store',
  logger: 'pretty'
})
