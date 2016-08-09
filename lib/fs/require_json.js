var cache = {}

/*
 * Works identically to require('/path/to/file.json'), but safer.
 */

module.exports = function requireJson (path, opts) {
  opts = opts || {}
  path = require('path').resolve(path)
  if (!opts.ignoreCache && cache[path]) return cache[path]
  cache[path] = JSON.parse(require('fs').readFileSync(path, 'utf-8'))
  return cache[path]
}
