'use strict'
const path = require('path')
const seq = require('promisequence')
const chalk = require('chalk')

const createGot = require('../network/got')
const pnpmPkgJson = require('../../package.json')
const initCmd = require('./init_cmd')
const installMultiple = require('../install_multiple')
const save = require('../save')
const linkPeers = require('../install/link_peers')
const runtimeError = require('../runtime_error')
const getSaveType = require('../get_save_type')
const runScript = require('../run_script')
const postInstall = require('../install/post_install')

/*
 * Perform installation.
 *
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { quiet: true })
 */

function install (packagesToInstall, opts) {
  packagesToInstall = mapify(packagesToInstall)
  const installType = packagesToInstall && Object.keys(packagesToInstall).length ? 'named' : 'general'
  opts = Object.assign({}, require('../defaults'), opts)

  let cmd
  const isProductionInstall = opts.production || process.env.NODE_ENV === 'production'

  return initCmd(opts)
    .then(_ => { cmd = _ })
    .then(_ => install())
    .then(_ => linkPeers(cmd.pkg, cmd.ctx.store, cmd.ctx.installs))
    // postinstall hooks
    .then(_ => {
      if (cmd.ctx.ignoreScripts || !cmd.ctx.piq || !cmd.ctx.piq.length) return
      return seq(
        cmd.ctx.piq.map(pkg => () =>
          postInstall(pkg.path, installLogger(pkg.pkgFullname))
            .catch(err => {
              if (cmd.ctx.installs[pkg.pkgFullname].optional) {
                console.log('Skipping failed optional dependency ' + pkg.pkgFullname + ':')
                console.log(err.message || err)
                return
              }
              throw err
            })
        )
      )
    })
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

    cmd.ctx.got = createGot({
      concurrency: opts.concurrency,
      fetchRetries: opts.fetchRetries,
      fetchRetryFactor: opts.fetchRetryFactor,
      fetchRetryMintimeout: opts.fetchRetryMintimeout,
      fetchRetryMaxtimeout: opts.fetchRetryMaxtimeout
    })
    return installMultiple(cmd.ctx,
        packagesToInstall,
        cmd.pkg.pkg && cmd.pkg.pkg.optionalDependencies,
        path.join(cmd.ctx.root, 'node_modules'),
        Object.assign({}, opts, { dependent: cmd.pkg && cmd.pkg.path })
      )
      .then(savePkgs)
      .then(_ => cmd.storeJsonCtrl.save({
        pnpm: pnpmPkgJson.version,
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
    if (cmd.ctx.ignoreScripts) return
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

function installLogger (pkgFullname) {
  return (stream, line) => {
    require('debug')('pnpm:post_install')(`${pkgFullname} ${line}`)

    if (stream === 'stderr') {
      console.log(chalk.blue(pkgFullname) + '! ' + chalk.gray(line))
      return
    }
    console.log(chalk.blue(pkgFullname) + '  ' + chalk.gray(line))
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
