import path = require('path')
import seq = require('promisequence')
import chalk = require('chalk')
import createDebug = require('debug')
import RegClient = require('npm-registry-client')
import logger = require('@zkochan/logger')
import {PnpmOptions, StrictPnpmOptions, Dependencies} from '../types'
import createGot from '../network/got'
import initCmd, {CommandContext, CommandNamespace} from './initCmd'
import installMultiple from '../installMultiple'
import save from '../save'
import linkPeers from '../install/linkPeers'
import runtimeError from '../runtimeError'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import linkBins from '../install/linkBins'
import extendOptions from './extendOptions'
import {InstalledPackage} from '../install'
import {Got} from '../network/got'
import pnpmPkgJson from '../pnpmPkgJson'

export type PackageInstallationResult = {
  path: string,
  pkgId: string
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

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const cmd = await createInstallCmd(opts)

  try {
    if (!cmd.pkg || !cmd.pkg.pkg) throw runtimeError('No package.json found')
    const packagesToInstall = Object.assign({}, cmd.pkg.pkg.dependencies || {})
    if (!opts.production) Object.assign(packagesToInstall, cmd.pkg.pkg.devDependencies || {})

    await installInContext('general', packagesToInstall, cmd, opts)
    await cmd.unlock()
  } catch (err) {
    if (cmd && cmd.unlock) cmd.unlock()
    throw err
  }
}

/**
 * Perform installation.
 * 
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { quiet: true })
 */
export async function installPkgs (fuzzyDeps: string[] | Dependencies, maybeOpts?: PnpmOptions) {
  let packagesToInstall = mapify(fuzzyDeps)
  if (!Object.keys(packagesToInstall).length) {
    throw new Error('At least one package has to be installed')
  }
  const opts = extendOptions(maybeOpts)
  const cmd = await createInstallCmd(opts)

  try {
    await installInContext('named', packagesToInstall, cmd, opts)
    await cmd.unlock()
  } catch (err) {
    if (cmd && cmd.unlock) cmd.unlock()
    throw err
  }
}

async function installInContext (installType: string, packagesToInstall: Dependencies, cmd: InstallNamespace, opts: StrictPnpmOptions) {
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
          .then(() => postInstall(pkg.path, installLogger(pkg.pkgId)))
          .catch(err => {
            if (cmd.ctx.installs[pkg.pkgId].optional) {
              console.log('Skipping failed optional dependency ' + pkg.pkgId + ':')
              console.log(err.message || err)
              return
            }
            throw err
          })
      ))
  }
  await linkBins(path.join(cmd.ctx.root, 'node_modules'))
  if (!opts.ignoreScripts && cmd.pkg) {
    await mainPostInstall(cmd.pkg.pkg && cmd.pkg.pkg.scripts || {}, cmd.ctx.root, opts.production)
  }
}

async function createInstallCmd (opts: StrictPnpmOptions): Promise<InstallNamespace> {
  const baseCmd = await initCmd(opts)
  const client = new RegClient(adaptConfig(opts))
  const cmd: InstallNamespace = Object.assign(baseCmd, {
    ctx: Object.assign({}, baseCmd.ctx, {
      fetches: {},
      builds: {},
      installs: {},
      got: createGot(client)
    })
  })
  return cmd
}

function adaptConfig (opts: StrictPnpmOptions) {
  return {
    proxy: {
      http: opts.proxy,
      https: opts.httpsProxy,
      localAddress: opts.localAddress
    },
    ssl: {
      certificate: opts.cert,
      key: opts.key,
      ca: opts.ca,
      strict: opts.strictSsl
    },
    retry: {
      count: opts.fetchRetries,
      factor: opts.fetchRetryFactor,
      minTimeout: opts.fetchRetryMintimeout,
      maxTimeout: opts.fetchRetryMaxtimeout
    },
    userAgent: opts.userAgent,
    log: Object.assign({}, logger, {
      verbose: logger.log.bind(null, 'verbose'),
      http: logger.log.bind(null, 'http')
    }),
    defaultTag: opts.tag
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

function installLogger (pkgId: string) {
  return (stream: string, line: string) => {
    createDebug('pnpm:post_install')(`${pkgId} ${line}`)

    if (stream === 'stderr') {
      console.log(chalk.blue(pkgId) + '! ' + chalk.gray(line))
      return
    }
    console.log(chalk.blue(pkgId) + '  ' + chalk.gray(line))
  }
}

function mapify (pkgs: string[] | Dependencies): Dependencies {
  if (!pkgs) return {}
  if (Array.isArray(pkgs)) {
    return pkgs.reduce((pkgsMap: Dependencies, pkgRequest: string) => {
      const matches = /(@?[^@]+)@(.*)/.exec(pkgRequest)
      if (!matches) {
        pkgsMap[pkgRequest] = '*'
      } else {
        pkgsMap[matches[1]] = matches[2]
      }
      return pkgsMap
    }, {})
  }
  return pkgs
}
