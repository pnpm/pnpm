'use strict'
const requireJson = require('./fs/require_json')
const writeJson = require('./fs/write_json')

module.exports = (pkg, removedPackages, saveType) => {
  const packageJson = requireJson(pkg.path)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return Promise.resolve()

  removedPackages.forEach(dependency => {
    delete packageJson[saveType][dependency]
  })

  return writeJson(pkg.path, packageJson)
}
