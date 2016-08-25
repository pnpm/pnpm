'use strict'
const delimiter = '+'

module.exports = pkg => pkg.name.replace('/', delimiter) + '@' + escapeVersion(pkg.version)
module.exports.delimiter = delimiter

function escapeVersion (version) {
  if (!version) return ''
  return version.replace(/[/\\:]/g, delimiter)
}
