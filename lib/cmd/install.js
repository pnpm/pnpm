var dirname = require('path').dirname
var join = require('path').join
var assign = require('object-assign')
var npa = require('npm-package-arg')
var spawnSync = require('cross-spawn').sync

var initCmd = require('./init_cmd')
var installMultiple = require('../install_multiple')
var save = require('../save')
var linkPeers = require('../install/link_peers')
var runtimeError = require('../runtime_error')
var getSaveType = require('../get_save_type')

/*
 * Perform installation.
 *
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */

function installCmd (input, opts) {
  opts = Object.assign({}, require('../defaults'), opts)
  process.env.pnpm_config_concurrency = opts.concurrency

  var cmd
  var packagesToInstall
  var installType
  var isProductionInstall = opts.production || process.env.NODE_ENV === 'production'

  return initCmd(opts)
    .then(_ => { cmd = _ })
    .then(_ => install())
    .then(_ => linkPeers(cmd.pkg, cmd.ctx.store, cmd.ctx.installs))
    .then(_ => mainPostInstall())
    .then(_ => cmd.unlock())
    .catch(err => {
      cmd.unlock()
      throw err
    })

  function install () {
    installType = input && input.length ? 'named' : 'general'

    if (installType === 'named') {
      packagesToInstall = input
    } else {
      if (!cmd.pkg.pkg) throw runtimeError('No package.json found')
      packagesToInstall = assign({}, cmd.pkg.pkg.dependencies || {})
      if (!isProductionInstall) assign(packagesToInstall, cmd.pkg.pkg.devDependencies || {})
    }

    return installMultiple(cmd.ctx,
      packagesToInstall,
      cmd.pkg.pkg && cmd.pkg.pkg.optionalDependencies,
      join(cmd.ctx.root, 'node_modules'),
      Object.assign({}, opts, { dependent: cmd.pkg && cmd.pkg.path }))
      .then(savePkgs)
      .then(_ => cmd.storeJson.save({
        dependents: cmd.ctx.dependents,
        dependencies: cmd.ctx.dependencies
      }))
  }

  function savePkgs (packages) {
    var saveType = getSaveType(opts)
    if (saveType && installType === 'named') {
      var inputNames = input.map(pkgName => npa(pkgName).name)
      var savedPackages = packages.filter(pkg => inputNames.indexOf(pkg.name) > -1)
      return save(cmd.pkg, savedPackages, saveType, opts.saveExact)
    }
  }

  function mainPostInstall () {
    var scripts = cmd.pkg.pkg && cmd.pkg.pkg.scripts || {}
    if (scripts.postinstall) runScript('postinstall')
    if (!isProductionInstall && scripts.prepublish) runScript('prepublish')
    return

    function runScript (scriptName) {
      var result = spawnSync('npm', ['run', scriptName], {
        cwd: dirname(cmd.pkg.path),
        stdio: 'inherit'
      })
      if (result.status !== 0) {
        process.exit(result.status)
        return
      }
    }
  }
}

module.exports = installCmd
