'use strict'
const renameKey = require('./rename_key')

renameKey({
  currentKeyName: '__dependencies',
  newKeyName: 'dependencies',
  addPreinstall: false
})
