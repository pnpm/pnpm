'use strict'
const renameKeys = require('./rename_keys')

renameKeys({
  dependencies: '__dependencies',
  scripts: '__scripts'
})
