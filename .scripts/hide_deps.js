'use strict'
const renameKey = require('./rename_key')

renameKey({
  currentKeyName: 'dependencies',
  newKeyName: '__dependencies'
})
