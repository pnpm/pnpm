var readPkgUp = require('read-pkg-up')
var dirname = require('path').dirname
var join = require('path').join
var assign = require('object-assign')
var npa = require('npm-package-arg')

var logger = require('../logger')
var installMultiple = require('../install_multiple')
var config = require('../config')
var save = require('../save')
var linkPeers = require('../install/link_peers')

/*
 * Perform
 */

function run (cli) {
  var ctx = {}
  var pkg
  var packagesToInstall
  var installType

  return readPkgUp()
    .then(_ => { pkg = _ })
    .then(_ => updateContext(pkg.path))
    .then(_ => install())
    .then(_ => linkPeers(pkg, ctx.store, ctx.installs))

  function install () {
    installType = cli.input && cli.input.length ? 'named' : 'general'

    if (installType === 'named') {
      packagesToInstall = cli.input
    } else {
      packagesToInstall = assign({},
        pkg.pkg.dependencies || {},
        pkg.pkg.devDependencies || {})
    }

    return installMultiple(ctx,
      packagesToInstall,
      join(ctx.root, 'node_modules'),
      cli.flags)
      .then(savePkgs)
  }

  function updateContext (packageJson) {
    var root = packageJson ? dirname(packageJson) : process.cwd()
    ctx.root = root
    ctx.store = join(root, config.store_path)
    if (!cli.flags.quiet) ctx.log = logger()
    else ctx.log = function () { return function () {} }
  }

  function savePkgs (packages) {
    var saveType = getSaveType(cli.flags)
    if (saveType && installType === 'named') {
      var inputNames = cli.input.map(pkgName => npa(pkgName).name)
      var savedPackages = packages.filter(pkg => inputNames.indexOf(pkg.name) > -1)
      return save(pkg, savedPackages, saveType, cli.flags.saveExact)
    }
  }
}

function getSaveType (flags) {
  if (flags.save) return 'dependencies'
  if (flags.saveDev) return 'devDependencies'
  if (flags.saveOptional) return 'optionalDependencies'
}

module.exports = run
