'use strict'
const semver = require('semver')
const join = require('path').join
const fs = require('mz/fs')

/*
 * Check if a module exists (eg, `node_modules/node-pre-gyp`). This is the case when
 * it's part of bundleDependencies.
 *
 * This check is also responsible for stopping `pnpm i lodash` from doing anything when
 * 'node_modules/lodash' already exists.
 *
 *     spec = { name: 'lodash', spec: '^3.0.2' }
 *     isAvailable(spec, 'path/to/node_modules')
 */

module.exports = function isAvailable (spec, modules) {
  const name = spec && spec.name
  if (!name) return Promise.resolve(false)

  const packageJsonPath = join(modules, name, 'package.json')

  return Promise.resolve()
    .then(_ => fs.readFile(packageJsonPath))
    .then(_ => JSON.parse(_))
    .then(_ => verify(spec, _))
    .catch(err => {
      if (err.code !== 'ENOENT') throw err
      return false
    })

  function verify (spec, packageJson) {
    return packageJson.name === spec.name &&
      ((spec.type !== 'range' && spec.type !== 'version') ||
      semver.satisfies(packageJson.version, spec.spec))
  }
}
