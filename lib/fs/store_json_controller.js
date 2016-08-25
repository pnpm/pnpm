'use strict'
const join = require('path').join
const writeFileSync = require('fs').writeFileSync
const readFileSync = require('fs').readFileSync

module.exports = function storeJsonController (storePath) {
  const storeJsonPath = join(storePath, 'store.json')

  return {
    read () {
      try {
        return JSON.parse(readFileSync(storeJsonPath, 'utf8'))
      } catch (err) {
        return null
      }
    },
    save (storeJson) {
      writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
    }
  }
}
