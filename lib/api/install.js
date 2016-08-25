'use strict'
const path = require('path')

const initCmd = require('./init_cmd')
const installMultiple = require('../install_multiple')
const save = require('../save')
const linkPeers = require('../install/link_peers')
const runtimeError = require('../runtime_error')
const getSaveType = require('../get_save_type')
const runScript = require('../run_script')

/*
 * Perform installation.
 *
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { quiet: true })
 */

function install (packagesToInstall, opts) {
  packagesToInstall = mapify(packagesToInstall)
  const installType = packagesToInstall && Object.keys(packagesToInstall).length ? 'named' : 'general'
  opts = Object.assign({}, require('../defaults'), opts)
  process.env.pnpm_config_concurrency = opts.concurrency

  let cmd
  const isProductionInstall = opts.production || process.env.NODE_ENV === 'production'

  return initCmd(opts)
    .then(_ => { cmd = _ })
    .then(_ => install())
    .then(_ => linkPeers(cmd.pkg, cmd.ctx.store, cmd.ctx.installs))
    .then(_ => mainPostInstall())
    .then(_ => cmd.unlock())
    .catch(err => {
      if (cmd && cmd.unlock) cmd.unlock()
      throw err
    })

  function install () {
    if (installType !== 'named') {
      if (!cmd.pkg.pkg) throw runtimeError('No package.json found')
      packagesToInstall = Object.assign({}, cmd.pkg.pkg.dependencies || {})
      if (!isProductionInstall) Object.assign(packagesToInstall, cmd.pkg.pkg.devDependencies || {})
    }

    return installMultiple(cmd.ctx,
        packagesToInstall,
        cmd.pkg.pkg && cmd.pkg.pkg.optionalDependencies,
        path.join(cmd.ctx.root, 'node_modules'),
        Object.assign({}, opts, { dependent: cmd.pkg && cmd.pkg.path })
      )
      .then(savePkgs)
      .then(_ => cmd.storeJson.save({
        dependents: cmd.ctx.dependents,
        dependencies: cmd.ctx.dependencies
      }))
  }

  function savePkgs (packages) {
    const saveType = getSaveType(opts)
    if (saveType && installType === 'named') {
      const inputNames = Object.keys(packagesToInstall)
      const savedPackages = packages.filter(pkg => inputNames.indexOf(pkg.name) > -1)
      return save(cmd.pkg, savedPackages, saveType, opts.saveExact)
    }
  }

  function mainPostInstall () {
    const scripts = cmd.pkg.pkg && cmd.pkg.pkg.scripts || {}
    if (scripts.postinstall) npmRun('postinstall')
    if (!isProductionInstall && scripts.prepublish) npmRun('prepublish')
    return

    function npmRun (scriptName) {
      const result = runScript.sync('npm', ['run', scriptName], {
        cwd: path.dirname(cmd.pkg.path),
        stdio: 'inherit'
      })
      if (result.status !== 0) {
        process.exit(result.status)
        return
      }
    }
  }
}

function mapify (pkgs) {
  if (!pkgs) return {}
  if (Array.isArray(pkgs)) {
    return pkgs.reduce((pkgsMap, pkgFullName) => {
      const matches = /(@?[^@]+)@(.*)/.exec(pkgFullName)
      if (!matches) {
        pkgsMap[pkgFullName] = null
      } else {
        pkgsMap[matches[1]] = matches[2]
      }
      return pkgsMap
    }, {})
  }
  return pkgs
}

module.exports = install
