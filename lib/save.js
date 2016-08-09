var requireJson = require('./fs/require_json')
var writeJson = require('./fs/write_json')
var sortedObject = require('sorted-object')

module.exports = function save (pkg, installedPackages, saveType, useExactVersion) {
  // Read the latest version of package.json to avoid accidental overwriting
  var packageJson = requireJson(pkg.path, { ignoreCache: true })
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(function (dependency) {
    var semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })
  packageJson[saveType] = sortedObject(packageJson[saveType])

  return writeJson(pkg.path, packageJson)
}
