'use strict'
const renameKey = require('./rename_key')

renameKey({
  pkgName: 'pnpm',
  currentKeyName: '__dependencies',
  newKeyName: 'dependencies',
  addPreinstall: false
})
