var writeFile = require('mz/fs').writeFile

module.exports = function save (pkg, installedPackages, saveType, useExactVersion) {
  var packageJson = pkg.pkg
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(function (dependency) {
    var semverCharacter = useExactVersion ? '' : '^'
    packageJson[saveType][dependency.spec.name] = semverCharacter + dependency.version
  })

  return writeFile(pkg.path, JSON.stringify(packageJson, null, 2), 'utf8')
}
