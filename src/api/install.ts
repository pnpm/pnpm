import path = require('path')
import RegClient = require('npm-registry-client')
import logger from 'pnpm-logger'
import pLimit = require('p-limit')
import npa = require('npm-package-arg')
import symlinkDir from 'symlink-dir'
import pFilter = require('p-filter')
import getLinkTarget = require('get-link-target')
import {PnpmOptions, StrictPnpmOptions, Dependencies} from '../types'
import createGot from '../network/got'
import getContext, {PnpmContext} from './getContext'
import installMultiple, {InstalledPackage} from '../install/installMultiple'
import linkPackages from '../link'
import save from '../save'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import extendOptions from './extendOptions'
import pnpmPkgJson from '../pnpmPkgJson'
import lock from './lock'
import {
  save as saveShrinkwrap,
  prune as pruneShrinkwrap,
  Shrinkwrap,
  ResolvedDependencies,
  pkgIdToRef,
} from '../fs/shrinkwrap'
import {save as saveModules} from '../fs/modulesController'
import removeOrphanPkgs from './removeOrphanPkgs'
import mkdirp = require('mkdirp-promise')
import createMemoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {PackageSpec} from '../resolve'
import depsToSpecs from '../depsToSpecs'

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type InstallContext = {
  installs: InstalledPackages,
  installationSequence: string[],
  shrinkwrap: Shrinkwrap,
  installed: Set<string>,
  fetchingLocker: MemoizedFunc<Boolean>,
}

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.shrinkwrap)

  if (!ctx.pkg) throw new Error('No package.json found')
  const optionalDeps = ctx.pkg.optionalDependencies || {}
  const depsToInstall = Object.assign(
    {},
    !opts.production && ctx.pkg.devDependencies,
    optionalDeps,
    ctx.pkg.dependencies
  )
  const specs = depsToSpecs(depsToInstall)

  return lock(
    ctx.storePath,
    () => installInContext('general', specs, Object.keys(optionalDeps), ctx, installCtx, opts),
    {stale: opts.lockStaleDuration}
  )
}

/**
 * Perform installation.
 *
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { silent: true })
 */
export async function installPkgs (fuzzyDeps: string[] | Dependencies, maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  let packagesToInstall = Array.isArray(fuzzyDeps)
    ? argsToSpecs(fuzzyDeps, opts.tag)
    : depsToSpecs(fuzzyDeps)

  if (!Object.keys(packagesToInstall).length) {
    throw new Error('At least one package has to be installed')
  }
  const ctx = await getContext(opts)
  const installCtx = await createInstallCmd(opts, ctx.shrinkwrap)

  return lock(
    ctx.storePath,
    () => installInContext('named', packagesToInstall, [], ctx, installCtx, opts),
    {stale: opts.lockStaleDuration}
  )
}

function argsToSpecs (args: string[], defaultTag: string): PackageSpec[] {
  return args
    .map(arg => npa(arg))
    .map(spec => {
      if (spec.type === 'tag' && !spec.raw.endsWith('@latest')) {
        spec.spec = defaultTag
      }
      return spec
    })
}

function getResolutions(
  packagesToInstall: PackageSpec[],
  resolvedSpecDeps: ResolvedDependencies
): ResolvedDependencies {
  return packagesToInstall
    .reduce((resolvedDeps, depSpec) => {
      if (resolvedSpecDeps[depSpec.raw]) {
        resolvedDeps[depSpec.name] = resolvedSpecDeps[depSpec.raw]
      }
      return resolvedDeps
    }, {})
}

async function installInContext (
  installType: string,
  packagesToInstall: PackageSpec[],
  optionalDependencies: string[],
  ctx: PnpmContext,
  installCtx: InstallContext,
  opts: StrictPnpmOptions
) {
  const nodeModulesPath = path.join(ctx.root, 'node_modules')
  const client = new RegClient(adaptConfig(opts))

  const resolvedDependencies: ResolvedDependencies | undefined = installType !== 'general'
    ? undefined
    : getResolutions(
      packagesToInstall,
      ctx.shrinkwrap.dependencies
    )
  const installOpts = {
    root: ctx.root,
    storePath: ctx.storePath,
    localRegistry: opts.localRegistry,
    registry: ctx.shrinkwrap.registry,
    force: opts.force,
    depth: opts.depth,
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    got: createGot(client, {networkConcurrency: opts.networkConcurrency}),
    metaCache: opts.metaCache,
    resolvedDependencies,
    offline: opts.offline,
  }
  const nonLinkedPkgs = await pFilter(packagesToInstall, (spec: PackageSpec) => !spec.name || isInnerLink(nodeModulesPath, spec.name))
  const pkgs: InstalledPackage[] = await installMultiple(
    installCtx,
    nonLinkedPkgs,
    optionalDependencies,
    installOpts
  )
  const linkedPkgsMap = await linkPackages(pkgs, installCtx.installs, {
    force: opts.force,
    global: opts.global,
    baseNodeModules: nodeModulesPath,
  })

  let newPkg: Package | undefined = ctx.pkg
  if (installType === 'named') {
    if (!ctx.pkg) {
      throw new Error('Cannot save because no package.json found')
    }
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    const saveType = getSaveType(opts)
    newPkg = await save(pkgJsonPath, pkgs.map(pkg => pkg.pkg), saveType, opts.saveExact)
  }

  if (newPkg) {
    ctx.shrinkwrap.dependencies = ctx.shrinkwrap.dependencies || {}

    const deps = newPkg.dependencies || {}
    const devDeps = newPkg.devDependencies || {}
    const optionalDeps = newPkg.optionalDependencies || {}

    const getSpecFromPkg = (depName: string) => deps[depName] || devDeps[depName] || optionalDeps[depName]

    pkgs.forEach(dep => {
      const spec = getSpecFromPkg(dep.pkg.name)
      if (spec) {
        ctx.shrinkwrap.dependencies[`${dep.pkg.name}@${spec}`] = pkgIdToRef(dep.id, dep.pkg.version, dep.resolution, ctx.shrinkwrap.registry)
      }
    })
    Object.keys(ctx.shrinkwrap.dependencies)
      .map(npa)
      .filter((depSpec: PackageSpec) => getSpecFromPkg(depSpec.name) !== depSpec.rawSpec)
      .map((depSpec: PackageSpec) => depSpec.raw)
      .forEach(removedDep => {
        delete ctx.shrinkwrap.dependencies[removedDep]
      })
  }

  const newShr = pruneShrinkwrap(ctx.shrinkwrap)
  await removeOrphanPkgs(ctx.privateShrinkwrap, newShr, ctx.root, ctx.storePath)
  await saveShrinkwrap(ctx.root, newShr)
  if (ctx.isFirstInstallation) {
    await saveModules(path.join(ctx.root, 'node_modules'), {
      packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
      storePath: ctx.storePath,
    })
  }

  // postinstall hooks
  if (!(opts.ignoreScripts || !installCtx.installationSequence || !installCtx.installationSequence.length)) {
    const limitChild = pLimit(opts.childConcurrency)
    await Promise.all(
      installCtx.installationSequence.map(pkgId => limitChild(async () => {
        try {
          await postInstall(linkedPkgsMap[pkgId].hardlinkedLocation, installLogger(pkgId))
        } catch (err) {
          if (installCtx.installs[pkgId].optional) {
            logger.warn({
              message: `Skipping failed optional dependency ${pkgId}`,
              err,
            })
            return
          }
          throw err
        }
      }))
    )
  }
  if (!opts.ignoreScripts && ctx.pkg) {
    const scripts = ctx.pkg && ctx.pkg.scripts || {}

    if (scripts['postinstall']) {
      npmRun('postinstall', ctx.root)
    }
    if (installType === 'general' && scripts['prepublish']) {
      npmRun('prepublish', ctx.root)
    }
  }
}

async function isInnerLink (modules: string, depName: string) {
  let linkTarget: string
  try {
    const linkPath = path.join(modules, depName)
    linkTarget = await getLinkTarget(linkPath)
  } catch (err) {
    if (err.code === 'ENOENT') return true
    throw err
  }

  if (linkTarget.startsWith(modules)) {
    return true
  }
  logger.info(`${depName} is linked to ${modules} from ${linkTarget}`)
  return false
}

async function createInstallCmd (opts: StrictPnpmOptions, shrinkwrap: Shrinkwrap): Promise<InstallContext> {
  return {
    installs: {},
    shrinkwrap,
    installed: new Set(),
    installationSequence: [],
    fetchingLocker: createMemoize<boolean>(opts.fetchingConcurrency),
  }
}

function adaptConfig (opts: StrictPnpmOptions) {
  const registryLog = logger('registry')
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
    log: Object.assign({}, registryLog, {
      verbose: registryLog.debug.bind(null, 'http'),
      http: registryLog.debug.bind(null, 'http'),
    }),
    defaultTag: opts.tag
  }
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

const lifecycleLogger = logger('lifecycle')

function installLogger (pkgId: string) {
  return (stream: string, line: string) => {
    const logLevel = stream === 'stderr' ? 'error' : 'info'
    lifecycleLogger[logLevel]({pkgId, line})
  }
}
