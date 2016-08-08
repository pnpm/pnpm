var join = require('path').join
var writeFileSync = require('fs').writeFileSync
var readFileSync = require('fs').readFileSync

module.exports = function storeJsonController (storePath) {
  var storeJsonPath = join(storePath, 'store.json')

  return {
    read: function () {
      try {
        return JSON.parse(readFileSync(storeJsonPath, 'utf8'))
      } catch (err) {
        return {}
      }
    },
    save: function (storeJson) {
      writeFileSync(storeJsonPath, JSON.stringify(storeJson, null, 2), 'utf8')
    }
  }
}
