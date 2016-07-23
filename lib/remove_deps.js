var requireJson = require('./fs/require_json')
var writeJson = require('./fs/write_json')

module.exports = function (pkg, removedPackages, saveType) {
  var packageJson = requireJson(pkg.path)
  packageJson[saveType] = packageJson[saveType]
  if (!packageJson[saveType]) return Promise.resolve()

  removedPackages.forEach(function (dependency) {
    delete packageJson[saveType][dependency]
  })

  return writeJson(pkg.path, packageJson)
}
