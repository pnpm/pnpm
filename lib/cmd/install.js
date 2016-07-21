var readPkgUp = require('read-pkg-up')
var dirname = require('path').dirname
var join = require('path').join
var resolve = require('path').resolve
var assign = require('object-assign')
var npa = require('npm-package-arg')
var spawnSync = require('cross-spawn').sync

var logger = require('../logger')
var installMultiple = require('../install_multiple')
var save = require('../save')
var linkPeers = require('../install/link_peers')
var runtimeError = require('../runtime_error')

/*
 * Perform installation.
 *
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */

function installCmd (input, opts) {
  opts = Object.assign({}, opts, {
    concurrency: 16,
    store_path: 'node_modules/.store',
    logger: 'pretty'
  })
  process.env.pnpm_config_concurrency = opts.concurrency

  var ctx = {}
  var pkg
  var packagesToInstall
  var installType
  var isProductionInstall = opts.production || process.env.NODE_ENV === 'production'

  return readPkgUp()
    .then(_ => { pkg = _ })
    .then(_ => updateContext(pkg.path))
    .then(_ => install())
    .then(_ => linkPeers(pkg, ctx.store, ctx.installs))
    .then(_ => mainPostInstall())

  function install () {
    installType = input && input.length ? 'named' : 'general'

    if (installType === 'named') {
      packagesToInstall = input
    } else {
      if (!pkg.pkg) throw runtimeError('No package.json found')
      packagesToInstall = assign({}, pkg.pkg.dependencies || {})
      if (!isProductionInstall) assign(packagesToInstall, pkg.pkg.devDependencies || {})
    }

    return installMultiple(ctx,
      packagesToInstall,
      pkg.pkg && pkg.pkg.optionalDependencies,
      join(ctx.root, 'node_modules'),
      opts)
      .then(savePkgs)
  }

  function updateContext (packageJson) {
    var root = packageJson ? dirname(packageJson) : process.cwd()
    ctx.root = root
    ctx.store = resolve(root, opts.store_path)
    if (!opts.quiet) ctx.log = logger(opts.logger)
    else ctx.log = function () { return function () {} }
  }

  function savePkgs (packages) {
    var saveType = getSaveType(opts)
    if (saveType && installType === 'named') {
      var inputNames = input.map(pkgName => npa(pkgName).name)
      var savedPackages = packages.filter(pkg => inputNames.indexOf(pkg.name) > -1)
      return save(pkg, savedPackages, saveType, opts.saveExact)
    }
  }

  function mainPostInstall () {
    var scripts = pkg.pkg && pkg.pkg.scripts || {}
    if (scripts.postinstall) runScript('postinstall')
    if (!isProductionInstall && scripts.prepublish) runScript('prepublish')
    return

    function runScript (scriptName) {
      var result = spawnSync('npm', ['run', scriptName], {
        cwd: dirname(pkg.path),
        stdio: 'inherit'
      })
      if (result.status !== 0) {
        process.exit(result.status)
        return
      }
    }
  }
}

function getSaveType (opts) {
  if (opts.save) return 'dependencies'
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
}

module.exports = installCmd
