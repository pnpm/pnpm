var cbRimraf = require('rimraf')
var join = require('path').join

var initCmd = require('./init_cmd')
var getSaveType = require('../get_save_type')
var removeDeps = require('../remove_deps')
var binify = require('../binify')

function uninstallCmd (input, opts) {
  opts = Object.assign({}, require('../defaults'), opts)

  var cmd
  var uninstalledPkgs = []
  var saveType = getSaveType(opts)

  return initCmd(opts)
    .then(_ => { cmd = _ })
    .then(_ => {
      cmd.pkg.pkg.dependencies = cmd.pkg.pkg.dependencies || {}
      var pkgFullNames = input.map(dep => cmd.ctx.dependencies[cmd.pkg.path].find(_ => _.indexOf(dep + '@') === 0))
      tryUninstall(pkgFullNames.slice())
      if (cmd.ctx.dependencies[cmd.pkg.path]) {
        pkgFullNames.forEach(dep => {
          cmd.ctx.dependencies[cmd.pkg.path].splice(cmd.ctx.dependencies[cmd.pkg.path].indexOf(dep), 1)
        })
        if (!cmd.ctx.dependencies[cmd.pkg.path].length) {
          delete cmd.ctx.dependencies[cmd.pkg.path]
        }
      }
      return Promise.all(uninstalledPkgs.map(removePkgFromStore))
    })
    .then(_ => cmd.storeJson.save({
      dependents: cmd.ctx.dependents,
      dependencies: cmd.ctx.dependencies
    }))
    .then(_ => Promise.all(input.map(dep => rimraf(join(cmd.ctx.root, 'node_modules', dep)))))
    .then(_ => saveType && removeDeps(cmd.pkg, input, saveType))
    .then(_ => cmd.unlock())
    .catch(err => {
      if (cmd && cmd.unlock) cmd.unlock()
      throw err
    })

  function canBeUninstalled (pkgFullName) {
    return !cmd.ctx.dependents[pkgFullName] || !cmd.ctx.dependents[pkgFullName].length ||
      cmd.ctx.dependents[pkgFullName].length === 1 && cmd.ctx.dependents[pkgFullName].indexOf(cmd.pkg.path) !== -1
  }

  function tryUninstall (pkgFullNames) {
    do {
      var numberOfUninstalls = 0
      for (var i = 0; i < pkgFullNames.length;) {
        if (canBeUninstalled(pkgFullNames[i])) {
          var uninstalledPkg = pkgFullNames.splice(i, 1)[0]
          removeBins(uninstalledPkg)
          uninstalledPkgs.push(uninstalledPkg)
          var deps = cmd.ctx.dependencies[uninstalledPkg] || []
          delete cmd.ctx.dependencies[uninstalledPkg]
          delete cmd.ctx.dependents[uninstalledPkg]
          deps.forEach(dep => removeDependency(dep, uninstalledPkg))
          tryUninstall(deps)
          numberOfUninstalls++
          continue
        }
        i++
      }
    } while (numberOfUninstalls)
  }

  function removeDependency (dependentPkgName, uninstalledPkg) {
    if (!cmd.ctx.dependents[dependentPkgName]) return
    cmd.ctx.dependents[dependentPkgName].splice(cmd.ctx.dependents[dependentPkgName].indexOf(uninstalledPkg), 1)
    if (!cmd.ctx.dependents[dependentPkgName].length) {
      delete cmd.ctx.dependents[dependentPkgName]
    }
  }

  function removeBins (uninstalledPkg) {
    var uninstalledPkgJson = require(join(cmd.ctx.store, uninstalledPkg, '_/package.json'))
    var bins = binify(uninstalledPkgJson)
    Object.keys(bins).forEach(bin => cbRimraf.sync(join(cmd.ctx.root, 'node_modules/.bin', bin)))
  }

  function removePkgFromStore (pkgFullName) {
    return rimraf(join(cmd.ctx.store, pkgFullName))
  }

  function rimraf (filePath) {
    return new Promise((resolve, reject) => {
      cbRimraf(filePath, err => err ? reject(err) : resolve())
    })
  }
}

module.exports = uninstallCmd
