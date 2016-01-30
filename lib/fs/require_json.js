var cache = {}

/*
 * Works identically to require('/path/to/file.json'), but safer.
 */

module.exports = function requireJson (path) {
  path = require('path').resolve(path)
  if (cache[path]) return cache[path]
  cache[path] = JSON.parse(require('fs').readFileSync(path, 'utf-8'))
  return cache[path]
}
