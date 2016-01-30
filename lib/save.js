var thenify = require('thenify')
var fs = require('fs')
var writeFile = thenify(fs.writeFile)

module.exports = function save (pkg, installedPackages, saveType) {
  var packageJson = pkg.pkg
  packageJson[saveType] = packageJson[saveType] || {}
  installedPackages.forEach(function (dependency) {
    var spec = dependency.spec
    if (spec.spec === 'latest' && spec.rawSpec === '') {
      // Covers `npmn install express` (without defined version)
      spec.resolvedSpec = '^' + dependency.version
    }
    packageJson[saveType][spec.name] = spec.resolvedSpec || spec.spec
  })

  return writeFile(pkg.path, JSON.stringify(packageJson, null, 2), 'utf8')
}
