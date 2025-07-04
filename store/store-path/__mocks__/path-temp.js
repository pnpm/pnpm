const path = require('node:path')

module.exports = function (dir) {
  return path.join(dir, 'tmp')
}
