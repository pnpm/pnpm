import path = require('path')
import RegClient = require('npm-registry-client')
import logger from 'pnpm-logger'
import pLimit = require('p-limit')
import npa = require('npm-package-arg')
import pFilter = require('p-filter')
import R = require('ramda')
import safeIsInnerLink from '../safeIsInnerLink'
import safeReadPkg from '../fs/safeReadPkg'
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
  const installType = 'general'
  const ctx = await getContext(opts, installType)
  const installCtx = await createInstallCmd(opts, ctx.shrinkwrap)

  if (!ctx.pkg) throw new Error('No package.json found')

  const specs = specsToInstallFromPackage(ctx.pkg, {
    production: opts.production,
    prefix: opts.prefix,
  })

  const optionalDeps = R.keys(ctx.pkg.optionalDependencies)

  if (opts.lock === false) {
    return run()
  }

  return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

  async function run () {
    const scripts = !opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts || {}
    if (scripts['preinstall']) {
      npmRun('preinstall', ctx.root, opts.userAgent)
    }

    await installInContext(installType, specs, optionalDeps, [], ctx, installCtx, opts)

    if (scripts['postinstall']) {
      npmRun('postinstall', ctx.root, opts.userAgent)
    }
    if (scripts['prepublish']) {
      npmRun('prepublish', ctx.root, opts.userAgent)
    }
  }
}

function specsToInstallFromPackage(
  pkg: Package,
  opts: {
    production: boolean,
    prefix: string,
  }
): PackageSpec[] {
  const depsToInstall = depsToInstallFromPackage(pkg, {
    production: opts.production
  })
  return depsToSpecs(depsToInstall, opts.prefix)
}

function depsToInstallFromPackage(
  pkg: Package,
  opts: {
    production: boolean
  }
): Dependencies {
  return Object.assign(
    {},
    !opts.production && pkg.devDependencies || {},
    pkg.optionalDependencies,
    pkg.dependencies
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
    ? argsToSpecs(fuzzyDeps, opts.tag, opts.prefix)
    : depsToSpecs(fuzzyDeps, opts.prefix)

  if (!Object.keys(packagesToInstall).length) {
    throw new Error('At least one package has to be installed')
  }
  const installType = 'named'
  const ctx = await getContext(opts, installType)
  const installCtx = await createInstallCmd(opts, ctx.shrinkwrap)
  const optionalDependencies = opts.saveOptional
    ? packagesToInstall.map(spec => spec.name)
    : []

  if (opts.lock === false) {
    return run()
  }

  return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

  function run () {
    return installInContext(
      installType,
      packagesToInstall,
      optionalDependencies,
      packagesToInstall.map(spec => spec.name),
      ctx,
      installCtx,
      opts)
  }
}

function argsToSpecs (args: string[], defaultTag: string, where: string): PackageSpec[] {
  return args
    .map(arg => npa(arg, where))
    .map(spec => {
      if (spec.type === 'tag' && !spec.rawSpec) {
        spec.fetchSpec = defaultTag
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
  newPkgs: string[],
  ctx: PnpmContext,
  installCtx: InstallContext,
  opts: StrictPnpmOptions
) {
  const nodeModulesPath = path.join(ctx.root, 'node_modules')
  const client = new RegClient(adaptConfig(opts))

  const parts = R.partition(spec => newPkgs.indexOf(spec.name) === -1, packagesToInstall)
  const oldSpecs = parts[0]
  const newSpecs = parts[1]

  const resolvedDependencies: ResolvedDependencies = getResolutions(
    oldSpecs,
    ctx.shrinkwrap.dependencies
  )
  const installOpts = {
    root: ctx.root,
    storePath: ctx.storePath,
    localRegistry: opts.localRegistry,
    registry: ctx.shrinkwrap.registry,
    force: opts.force,
    depth: opts.update ? opts.depth :
      (R.equals(ctx.shrinkwrap.packages, ctx.privateShrinkwrap.packages) ? opts.repeatInstallDepth : Infinity),
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    got: createGot(client, {
      networkConcurrency: opts.networkConcurrency,
      rawNpmConfig: opts.rawNpmConfig,
      alwaysAuth: opts.alwaysAuth,
    }),
    metaCache: opts.metaCache,
    resolvedDependencies,
    offline: opts.offline,
    rawNpmConfig: opts.rawNpmConfig,
    nodeModules: nodeModulesPath,
    update: opts.update,
  }
  const nonLinkedPkgs = await pFilter(packagesToInstall, (spec: PackageSpec) => !spec.name || safeIsInnerLink(nodeModulesPath, spec.name))
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
    bin: opts.bin,
    topParents: ctx.pkg
      ? await getTopParents(
          R.difference(R.keys(depsToInstallFromPackage(ctx.pkg, {
            production: opts.production
          })), newPkgs), nodeModulesPath)
      : [],
  })

  let newPkg: Package | undefined = ctx.pkg
  if (installType === 'named') {
    if (!ctx.pkg) {
      throw new Error('Cannot save because no package.json found')
    }
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    const saveType = getSaveType(opts)
    newPkg = await save(
      pkgJsonPath,
      <any>pkgs.map(dep => { // tslint:disable-line
        const spec: PackageSpec = R.find(spec => spec.raw === dep.specRaw, newSpecs)
        if (!spec) return null
        return {
          name: dep.pkg.name,
          saveSpec: getSaveSpec(spec, dep, opts.saveExact)
        }
      }).filter(Boolean),
      saveType
    )
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
      .map(rawSpec => npa(rawSpec))
      .filter((depSpec: PackageSpec) => getSpecFromPkg(depSpec.name) !== depSpec.rawSpec)
      .map((depSpec: PackageSpec) => depSpec.raw)
      .forEach(removedDep => {
        delete ctx.shrinkwrap.dependencies[removedDep]
      })
  }

  const newShr = pruneShrinkwrap(ctx.shrinkwrap)
  await removeOrphanPkgs(ctx.privateShrinkwrap, newShr, ctx.root, ctx.storePath)
  await saveShrinkwrap(ctx.root, newShr)
  await saveModules(path.join(ctx.root, 'node_modules'), {
    packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
    storePath: ctx.storePath,
    skipped: R.uniq(
      R.concat(
        ctx.skipped.filter(skippedPkgId => !installCtx.installed.has(skippedPkgId)),
        Array.from(installCtx.installed).filter(pkgId => !installCtx.installs[pkgId]))),
  })

  // postinstall hooks
  if (!(opts.ignoreScripts || !installCtx.installationSequence || !installCtx.installationSequence.length)) {
    const limitChild = pLimit(opts.childConcurrency)
    await Promise.all(
      installCtx.installationSequence.map(pkgId => Promise.all(
        R.uniqBy(linkedPkg => linkedPkg.hardlinkedLocation, R.values(linkedPkgsMap).filter(pkg => pkg.id === pkgId))
          .map(pkg => limitChild(async () => {
            try {
              await postInstall(pkg.hardlinkedLocation, installLogger(pkgId), {
                userAgent: opts.userAgent
              })
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
          })
        )))
    )
  }
}

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map(pkgName => path.join(modules, pkgName)).map(safeReadPkg)
  )
  return pkgs.filter(Boolean).map((pkg: Package) => ({
    name: pkg.name,
    version: pkg.version,
  }))
}

function getSaveSpec(spec: PackageSpec, pkg: InstalledPackage, saveExact: boolean) {
  switch (spec.type) {
    case 'version':
    case 'range':
    case 'tag':
      return `${saveExact ? '' : '^'}${pkg.pkg.version}`
    default:
      return spec.saveSpec
  }
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

function npmRun (scriptName: string, pkgRoot: string, userAgent: string) {
  const result = runScriptSync('npm', ['run', scriptName], {
    cwd: pkgRoot,
    stdio: 'inherit',
    userAgent,
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
