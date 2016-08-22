'use strict'
const renameKey = require('./rename_key')

renameKey({
  pkgName: 'pnpm-rocket',
  currentKeyName: 'dependencies',
  newKeyName: '__dependencies',
  addPreinstall: true
})
