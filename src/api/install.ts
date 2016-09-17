import path = require('path')
import seq = require('promisequence')
import chalk = require('chalk')
import createDebug = require('debug')

import createGot from '../network/got'
import initCmd, {CommandContext, CommandNamespace, BasicOptions, StrictBasicOptions} from './initCmd'
import installMultiple, {Dependencies} from '../installMultiple'
import save from '../save'
import linkPeers from '../install/linkPeers'
import runtimeError from '../runtimeError'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import linkBins from '../install/linkBins'
import defaults from '../defaults'
import {InstalledPackage} from '../install'
import {Got} from '../network/got'
import pnpmPkgJson from '../pnpmPkgJson'

export type PackageInstallationResult = {
  path: string,
  pkgFullname: string
}

export type CachedPromises = {
  [name: string]: Promise<void>
}

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type InstallContext = CommandContext & {
  installs: InstalledPackages,
  piq?: PackageInstallationResult[],
  got: Got,
  builds: CachedPromises,
  fetches: CachedPromises
}

export type InstallNamespace = CommandNamespace & {
  ctx: InstallContext
}

export type PublicInstallationOptions = BasicOptions & {
  save?: boolean,
  saveDev?: boolean,
  saveOptional?: boolean,
  production?: boolean,
  concurrency?: number,
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number,
  saveExact?: boolean,
  linkLocal?: boolean
}

export type StrictPublicInstallationOptions = StrictBasicOptions & {
  save: boolean,
  saveDev: boolean,
  saveOptional: boolean,
  production: boolean,
  concurrency: number,
  fetchRetries: number,
  fetchRetryFactor: number,
  fetchRetryMintimeout: number,
  fetchRetryMaxtimeout: number,
  saveExact: boolean,
  linkLocal: boolean
}

/*
 * Perform installation.
 *
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { quiet: true })
 */

export default async function (fuzzyDeps: string[] | Dependencies, optsNullable: PublicInstallationOptions) {
  let packagesToInstall = mapify(fuzzyDeps)
  const installType = packagesToInstall && Object.keys(packagesToInstall).length ? 'named' : 'general'
  const opts: StrictPublicInstallationOptions = Object.assign({}, defaults, optsNullable)

  const isProductionInstall = opts.production || process.env.NODE_ENV === 'production'

  const baseCmd = await initCmd(opts)
  const cmd: InstallNamespace = Object.assign(baseCmd, {
    ctx: Object.assign({}, baseCmd.ctx, {
      fetches: {},
      builds: {},
      installs: {},
      got: createGot({
        concurrency: opts.concurrency,
        fetchRetries: opts.fetchRetries,
        fetchRetryFactor: opts.fetchRetryFactor,
        fetchRetryMintimeout: opts.fetchRetryMintimeout,
        fetchRetryMaxtimeout: opts.fetchRetryMaxtimeout
      })
    })
  })

  try {
    if (installType !== 'named') {
      if (!cmd.pkg || !cmd.pkg.pkg) throw runtimeError('No package.json found')
      packagesToInstall = Object.assign({}, cmd.pkg.pkg.dependencies || {})
      if (!isProductionInstall) Object.assign(packagesToInstall, cmd.pkg.pkg.devDependencies || {})
    }
    const pkgs: InstalledPackage[] = await installMultiple(cmd.ctx,
      packagesToInstall,
      cmd.pkg && cmd.pkg.pkg && cmd.pkg.pkg.optionalDependencies || {},
      path.join(cmd.ctx.root, 'node_modules'),
      Object.assign({}, opts, { dependent: cmd.pkg && cmd.pkg.path || cmd.ctx.root })
    )

    if (installType === 'named') {
      const saveType = getSaveType(opts)
      if (saveType) {
        if (!cmd.pkg) {
          throw new Error('Cannot save because no package.json found')
        }
        const inputNames = Object.keys(packagesToInstall)
        const savedPackages = pkgs.filter((pkg: InstalledPackage) => inputNames.indexOf(pkg.pkg.name) > -1)
        await save(cmd.pkg.path, savedPackages, saveType, opts.saveExact)
      }
    }

    cmd.storeJsonCtrl.save(Object.assign(cmd.ctx.storeJson, {
      pnpm: pnpmPkgJson.version
    }))

    await linkPeers(cmd.ctx.store, cmd.ctx.installs)
    // postinstall hooks
    if (!(opts.ignoreScripts || !cmd.ctx.piq || !cmd.ctx.piq.length)) {
      await seq(
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
        ))
    }
    await linkBins(path.join(cmd.ctx.root, 'node_modules'))
    if (!opts.ignoreScripts && cmd.pkg) {
      await mainPostInstall(cmd.pkg.pkg && cmd.pkg.pkg.scripts || {}, cmd.ctx.root, isProductionInstall)
    }
    await cmd.unlock()
  } catch (err) {
    if (cmd && cmd.unlock) cmd.unlock()
    throw err
  }
}

function mainPostInstall (scripts: Object, pkgRoot: string, isProductionInstall: boolean) {
  if (scripts['postinstall']) npmRun('postinstall', pkgRoot)
  if (!isProductionInstall && scripts['prepublish']) npmRun('prepublish', pkgRoot)
}

function npmRun (scriptName: string, pkgRoot: string) {
  const result = runScriptSync('npm', ['run', scriptName], {
    cwd: pkgRoot,
    stdio: 'inherit'
  })
  if (result.status !== 0) {
    process.exit(result.status)
  }
}

function installLogger (pkgFullname: string) {
  return (stream: string, line: string) => {
    createDebug('pnpm:post_install')(`${pkgFullname} ${line}`)

    if (stream === 'stderr') {
      console.log(chalk.blue(pkgFullname) + '! ' + chalk.gray(line))
      return
    }
    console.log(chalk.blue(pkgFullname) + '  ' + chalk.gray(line))
  }
}

function mapify (pkgs: string[] | Dependencies): Dependencies {
  if (!pkgs) return {}
  if (Array.isArray(pkgs)) {
    return pkgs.reduce((pkgsMap: Dependencies, pkgFullName: string) => {
      const matches = /(@?[^@]+)@(.*)/.exec(pkgFullName)
      if (!matches) {
        pkgsMap[pkgFullName] = '*'
      } else {
        pkgsMap[matches[1]] = matches[2]
      }
      return pkgsMap
    }, {})
  }
  return pkgs
}
