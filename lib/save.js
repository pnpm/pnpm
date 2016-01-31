var writeFile = require('mz/fs').writeFile
var requireJson = require('./fs/require_json')

module.exports = function save (pkg, installedPackages, saveType, useExactVersion) {
  var packageJson = requireJson(pkg.path)
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(function (dependency) {
    var semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })

  return writeFile(pkg.path, JSON.stringify(packageJson, null, 2) + '\n', 'utf8')
}
