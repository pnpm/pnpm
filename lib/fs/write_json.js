var writeFile = require('mz/fs').writeFile

module.exports = function (path, json) {
  return writeFile(path, JSON.stringify(json, null, 2) + '\n', 'utf8')
}
