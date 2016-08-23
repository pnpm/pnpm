'use strict'
const requireJson = require('./fs/require_json')
const writeJson = require('./fs/write_json')
const sortedObject = require('sorted-object')

module.exports = function save (pkg, installedPackages, saveType, useExactVersion) {
  // Read the latest version of package.json to avoid accidental overwriting
  const packageJson = requireJson(pkg.path, { ignoreCache: true })
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(dependency => {
    const semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writeJson(pkg.path, packageJson)
}
