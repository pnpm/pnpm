var thenify = require('thenify')
var fs = require('fs')
var writeFile = thenify(fs.writeFile)

module.exports = function save (pkg, installedPackages, saveType) {
  var packageJson = pkg.pkg
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(function (dependency) {
    // Assumes package name of in the format of "package-name@x.x.x"
    // TODO: Support other install sources (Github, scoped pkgs etc.)
    var splitDep = dependency.split('@')
    var depName = splitDep[0]
    var depVersion = splitDep[1]
    packageJson[saveType][depName] = '^' + depVersion
  })

  return writeFile(pkg.path, JSON.stringify(packageJson, null, 2), 'utf8')
}
