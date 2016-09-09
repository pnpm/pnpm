import path = require('path')
import seq = require('promisequence')
import chalk = require('chalk')
import createDebug = require('debug')

import requireJson from '../fs/require_json'
import createGot from '../network/got'
import initCmd from './init_cmd'
import installMultiple from '../install_multiple'
import save from '../save'
import linkPeers from '../install/link_peers'
import runtimeError from '../runtime_error'
import getSaveType from '../get_save_type'
import {sync as runScriptSync} from '../run_script'
import postInstall from '../install/post_install'
import linkBins from '../install/link_bins'
import defaults from '../defaults'

const pnpmPkgJson = requireJson(path.resolve(__dirname, '../../package.json'))

/*
 * Perform installation.
 *
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { quiet: true })
 */

export default function install (packagesToInstall, opts) {
  packagesToInstall = mapify(packagesToInstall)
  const installType = packagesToInstall && Object.keys(packagesToInstall).length ? 'named' : 'general'
  opts = Object.assign({}, defaults, opts)

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
        cmd.ctx.piq.map(pkg => () => linkBins(path.join(pkg.path, '_', 'node_modules'))
            .then(() => postInstall(pkg.path, installLogger(pkg.pkgFullname)))
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
    .then(_ => linkBins(path.join(cmd.ctx.root, 'node_modules')))
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
      const result = runScriptSync('npm', ['run', scriptName], {
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
    createDebug('pnpm:post_install')(`${pkgFullname} ${line}`)

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
