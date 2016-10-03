'use strict'
const renameKeys = require('./rename_keys')

renameKeys({
  __dependencies: 'dependencies',
  __scripts: 'scripts'
})
