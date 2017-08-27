import path = require('path')
import RegClient = require('npm-registry-client')
import logger, {
  streamParser,
  lifecycleLogger,
  summaryLogger,
} from 'pnpm-logger'
import logStatus from '../logging/logInstallStatus'
import pLimit = require('p-limit')
import npa = require('npm-package-arg')
import pFilter = require('p-filter')
import R = require('ramda')
import safeIsInnerLink from '../safeIsInnerLink'
import {fromDir as safeReadPkgFromDir} from '../fs/safeReadPkg'
import {PnpmOptions, StrictPnpmOptions, Dependencies} from '../types'
import getContext, {PnpmContext} from './getContext'
import installMultiple, {InstalledPackage, PackageRequest} from '../install/installMultiple'
import externalLink from './link'
import linkPackages from '../link'
import save from '../save'
import getSaveType from '../getSaveType'
import {sync as runScriptSync} from '../runScript'
import postInstall from '../install/postInstall'
import extendOptions from './extendOptions'
import lock from './lock'
import {
  write as saveShrinkwrap,
  Shrinkwrap,
  ResolvedDependencies,
} from 'pnpm-shrinkwrap'
import {
  save as saveModules,
  LAYOUT_VERSION,
} from '../fs/modulesController'
import mkdirp = require('mkdirp-promise')
import createMemoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {ResolvedNode} from '../link/resolvePeers'
import depsToSpecs, {similarDepsToSpecs} from '../depsToSpecs'
import shrinkwrapsEqual from './shrinkwrapsEqual'
import {
  Got,
  createGot,
  Store,
  PackageContentInfo,
  PackageSpec,
  DirectoryResolution,
  Resolution,
  PackageMeta,
} from 'package-store'
import depsFromPackage from '../depsFromPackage'
import writePkg = require('write-pkg')
import Rx = require('@reactivex/rxjs')
import {PnpmError} from '../errorTypes'

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type TreeNode = {
  nodeId: string,
  children$: Rx.Observable<string>, // Node IDs of children
  pkg: InstalledPackage,
  depth: number,
  installable: boolean,
  isCircular: boolean,
}

export type TreeNodeMap = {
  [nodeId: string]: TreeNode,
}

export type InstallContext = {
  installs: InstalledPackages,
  processed: Set<string>,
  localPackages: {
    optional: boolean,
    dev: boolean,
    resolution: DirectoryResolution,
    absolutePath: string,
    version: string,
    name: string,
    specRaw: string,
  }[],
  shrinkwrap: Shrinkwrap,
  privateShrinkwrap: Shrinkwrap,
  fetchingLocker: {
    [pkgId: string]: {
      fetchingFiles: Promise<PackageContentInfo>,
      fetchingPkg: Promise<Package>,
      calculatingIntegrity: Promise<void>,
    },
  },
  // the IDs of packages that are not installable
  skipped: Set<string>,
  tree: {[nodeId: string]: TreeNode},
  storeIndex: Store,
  force: boolean,
  prefix: string,
  storePath: string,
  registry: string,
  metaCache: Map<string, PackageMeta>,
  got: Got,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  pnpmVersion: string,
  offline: boolean,
  rawNpmConfig: Object,
  nodeModules: string,
  verifyStoreInegrity: boolean,
  nonDevPackageIds: Set<string>,
  nonOptionalPackageIds: Set<string>,
}

export async function install (maybeOpts?: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = extendOptions(maybeOpts)

  if (opts.lock) {
    await lock(opts.prefix, _install, {stale: opts.lockStaleDuration, locks: opts.locks})
  } else {
    await _install()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _install() {
    const installType = 'general'
    const ctx = await getContext(opts, installType)

    if (!ctx.pkg) throw new Error('No package.json found')

    const specs = specsToInstallFromPackage(ctx.pkg, {
      prefix: opts.prefix,
    })

    if (ctx.shrinkwrap.specifiers) {
      ctx.shrinkwrap.dependencies = ctx.shrinkwrap.dependencies || {}
      ctx.shrinkwrap.devDependencies = ctx.shrinkwrap.devDependencies || {}
      ctx.shrinkwrap.optionalDependencies = ctx.shrinkwrap.optionalDependencies || {}
      for (const spec of specs) {
        if (ctx.shrinkwrap.specifiers[spec.name] !== spec.rawSpec) {
          delete ctx.shrinkwrap.dependencies[spec.name]
          delete ctx.shrinkwrap.devDependencies[spec.name]
          delete ctx.shrinkwrap.optionalDependencies[spec.name]
        }
      }
    }

    const scripts = !opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts || {}

    if (scripts['prepublish']) {
      logger.warn('`prepublish` scripts are deprecated. Use `prepare` for build steps and `prepublishOnly` for upload-only.')
    }

    if (scripts['preinstall']) {
      npmRun('preinstall', ctx.root, opts.userAgent)

      // a hack to avoid `npm run install` executing preinstall again
      if (scripts['install']) {
        const newScripts = Object.assign({}, scripts)
        delete newScripts['preinstall']
        const newPkg = Object.assign({}, ctx.pkg, {scripts: newScripts})
        await writePkg(path.join(ctx.root, 'package.json'), newPkg)
      }
    }

    if (opts.lock === false) {
      await run()
    } else {
      await lock(ctx.storePath, run, {stale: opts.lockStaleDuration, locks: opts.locks})
    }

    if (scripts['install']) {
      npmRun('install', ctx.root, opts.userAgent)

      // a hack to avoid `npm run install` executing preinstall again
      if (scripts['preinstall']) {
        const pkg = await safeReadPkgFromDir(ctx.root)
        if (pkg) {
          pkg.scripts = scripts
          await writePkg(path.join(ctx.root, 'package.json'), pkg)
        }
      }
    } else if (scripts['postinstall']) {
      npmRun('postinstall', ctx.root, opts.userAgent)
    }
    if (scripts['prepublish']) {
      npmRun('prepublish', ctx.root, opts.userAgent)
    }
    if (scripts['prepare']) {
      npmRun('prepare', ctx.root, opts.userAgent)
    }

    async function run () {
      await installInContext(installType, specs, [], ctx, opts)
    }
  }
}

function specsToInstallFromPackage(
  pkg: Package,
  opts: {
    prefix: string,
  }
): PackageSpec[] {
  const depsToInstall = depsFromPackage(pkg)
  return depsToSpecs(depsToInstall, {
    where: opts.prefix,
    optionalDependencies: pkg.optionalDependencies || {},
    devDependencies: pkg.devDependencies || {},
  })
}

/**
 * Perform installation.
 *
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { silent: true })
 */
export async function installPkgs (fuzzyDeps: string[] | Dependencies, maybeOpts?: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  maybeOpts = maybeOpts || {}
  if (maybeOpts.update === undefined) maybeOpts.update = true
  const opts = extendOptions(maybeOpts)

  if (opts.lock) {
    await lock(opts.prefix, _installPkgs, {stale: opts.lockStaleDuration, locks: opts.locks})
  } else {
    await _installPkgs()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _installPkgs () {
    const installType = 'named'
    const ctx = await getContext(opts, installType)
    const existingSpecs = opts.global ? {} : depsFromPackage(ctx.pkg)
    const saveType = getSaveType(opts)
    const optionalDependencies = saveType ? {} : ctx.pkg.optionalDependencies || {}
    const devDependencies = saveType ? {} : ctx.pkg.devDependencies || {}
    let packagesToInstall = Array.isArray(fuzzyDeps)
      ? argsToSpecs(fuzzyDeps, {
        defaultTag: opts.tag,
        where: opts.prefix,
        dev: opts.saveDev,
        optional: opts.saveOptional,
        existingSpecs,
        optionalDependencies,
        devDependencies,
      })
      : similarDepsToSpecs(fuzzyDeps, {
        where: opts.prefix,
        dev: opts.saveDev,
        optional: opts.saveOptional,
        existingSpecs,
        optionalDependencies,
        devDependencies,
      })

    if (!Object.keys(packagesToInstall).length) {
      throw new Error('At least one package has to be installed')
    }

    if (opts.lock === false) {
      return run()
    }

    return lock(ctx.storePath, run, {stale: opts.lockStaleDuration, locks: opts.locks})

    function run () {
      return installInContext(
        installType,
        packagesToInstall,
        packagesToInstall.map(spec => spec.name),
        ctx,
        opts)
    }
  }
}

function argsToSpecs (
  args: string[],
  opts: {
    defaultTag: string,
    where: string,
    dev: boolean,
    optional: boolean,
    existingSpecs: Dependencies,
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
  }
): PackageSpec[] {
  return args
    .map(arg => npa(arg, opts.where))
    .map(spec => {
      if (!spec.rawSpec && opts.existingSpecs[spec.name]) {
        return npa.resolve(spec.name, opts.existingSpecs[spec.name], opts.where)
      }
      if (spec.type === 'tag' && !spec.rawSpec) {
        spec.fetchSpec = opts.defaultTag
      }
      return spec
    })
    .map(spec => {
      spec.dev = opts.dev || !!opts.devDependencies[spec.name]
      spec.optional = opts.optional || !!opts.optionalDependencies[spec.name]
      return spec
    })
}

async function installInContext (
  installType: string,
  packagesToInstall: PackageSpec[],
  newPkgs: string[],
  ctx: PnpmContext,
  opts: StrictPnpmOptions
) {
  // Unfortunately, the private shrinkwrap file may differ from the public one.
  // A user might run named installations on a project that has a shrinkwrap.yaml file before running a noop install
  if (installType === 'named' && (
    ctx.existsPublicShrinkwrap && !ctx.existsPrivateShrinkwrap ||
    // TODO: this operation is quite expensive. We'll have to find a better solution to do this.
    // maybe in pnpm v2 it won't be needed. See: https://github.com/pnpm/pnpm/issues/841
    !shrinkwrapsEqual(ctx.privateShrinkwrap, ctx.shrinkwrap)) &&
    !R.equals(R.keys(depsFromPackage(ctx.pkg)).sort(), newPkgs.sort())
  ) {
    throw new PnpmError('OUT_OF_DATE_NODE_MODULES',
      `Can't do named installation when external shrinkwrap.yaml differs from internal (at node_modules/.shrinkwrap.yaml). Try to do an argumentless installation first (pnpm install).`)
  }

  const nodeModulesPath = path.join(ctx.root, 'node_modules')
  const client = new RegClient(adaptConfig(opts))

  const parts = R.partition(spec => newPkgs.indexOf(spec.name) === -1, packagesToInstall)
  const oldSpecs = parts[0]
  const newSpecs = parts[1]

  const installCtx: InstallContext = {
    installs: {},
    localPackages: [],
    shrinkwrap: ctx.shrinkwrap,
    privateShrinkwrap: ctx.privateShrinkwrap,
    fetchingLocker: {},
    skipped: ctx.skipped,
    tree: {},
    storeIndex: ctx.storeIndex,
    storePath: ctx.storePath,
    registry: ctx.shrinkwrap.registry,
    force: opts.force,
    depth: opts.update ? opts.depth :
      (R.equals(ctx.shrinkwrap.packages, ctx.privateShrinkwrap.packages) ? opts.repeatInstallDepth : Infinity),
    prefix: opts.prefix,
    offline: opts.offline,
    rawNpmConfig: opts.rawNpmConfig,
    nodeModules: nodeModulesPath,
    metaCache: opts.metaCache,
    verifyStoreInegrity: opts.verifyStoreIntegrity,
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    got: createGot(client, {
      networkConcurrency: opts.networkConcurrency,
      rawNpmConfig: opts.rawNpmConfig,
      alwaysAuth: opts.alwaysAuth,
      registry: opts.registry,
    }),
    nonDevPackageIds: new Set(),
    nonOptionalPackageIds: new Set(),
    processed: new Set(),
  }
  const installOpts = {
    root: ctx.root,
    resolvedDependencies: Object.assign({}, ctx.shrinkwrap.devDependencies, ctx.shrinkwrap.dependencies, ctx.shrinkwrap.optionalDependencies),
    update: opts.update,
    keypath: [],
    parentNodeId: ':/:',
    currentDepth: 0,
  }
  const nonLinkedPkgs = await pFilter(packagesToInstall,
    (spec: PackageSpec) => !spec.name || safeIsInnerLink(nodeModulesPath, spec.name, {storePath: ctx.storePath}))
  const packageRequests$ = installMultiple(
    installCtx,
    nonLinkedPkgs,
    installOpts
  )

  installCtx.tree = {}
  const rootPackageRequests$ = packageRequests$
    .take(nonLinkedPkgs.length)
    .do(packageRequest => {
      const nodeId = `:/:${packageRequest.pkgId}:`
      const pkg = installCtx.installs[packageRequest.pkgId]
      installCtx.tree[nodeId] = {
        nodeId,
        pkg,
        children$: buildTree(installCtx, nodeId, packageRequest.pkgId, 1, pkg.installable),
        depth: 0,
        installable: pkg.installable,
        isCircular: false,
      }
    })
    .shareReplay(Infinity)

  const rootNodeId$ = rootPackageRequests$.map(packageRequest => `:/:${packageRequest.pkgId}:`)

  let newPkg: Package | undefined = ctx.pkg
  if (installType === 'named') {
    if (!ctx.pkg) {
      throw new Error('Cannot save because no package.json found')
    }
    const pkgByRawSpec = await rootPackageRequests$
      .reduce((acc: {}, packageRequest: PackageRequest) => {
        acc[packageRequest.specRaw] = installCtx.installs[packageRequest.pkgId]
        return acc
      }, {})
      .toPromise()
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    const saveType = getSaveType(opts)
    newPkg = await save(
      pkgJsonPath,
      <any>newSpecs.map(spec => { // tslint:disable-line
        const dep = pkgByRawSpec[spec.raw] || R.find(lp => lp.specRaw === spec.raw, installCtx.localPackages)
        if (!dep) return null
        return {
          name: dep.name,
          saveSpec: getSaveSpec(spec, dep.version, opts.saveExact)
        }
      }).filter(Boolean),
      saveType
    )
  }

  const result = await linkPackages(rootNodeId$, installCtx.tree, {
    force: opts.force,
    global: opts.global,
    baseNodeModules: nodeModulesPath,
    bin: opts.bin,
    topParent$: ctx.pkg
      ? getTopParents(
          R.difference(R.keys(depsFromPackage(ctx.pkg)), newPkgs), nodeModulesPath)
      : Rx.Observable.empty(),
    shrinkwrap: ctx.shrinkwrap,
    production: opts.production,
    optional: opts.optional,
    root: ctx.root,
    privateShrinkwrap: ctx.privateShrinkwrap,
    storePath: ctx.storePath,
    skipped: ctx.skipped,
    pkg: newPkg || ctx.pkg,
    independentLeaves: opts.independentLeaves,
    storeIndex: ctx.storeIndex,
    nonDevPackageIds: installCtx.nonDevPackageIds,
    nonOptionalPackageIds: installCtx.nonOptionalPackageIds,
    localPackages: installCtx.localPackages,
  })

  await Promise.all([
    saveShrinkwrap(ctx.root, result.shrinkwrap, result.shrinkwrap),
    result.shrinkwrap.packages === undefined
      ? Promise.resolve()
      : saveModules(path.join(ctx.root, 'node_modules'), {
        packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
        store: ctx.storePath,
        skipped: Array.from(installCtx.skipped),
        layoutVersion: LAYOUT_VERSION,
        independentLeaves: opts.independentLeaves,
      }),
  ])

  // postinstall hooks
  if (!(opts.ignoreScripts || !result.updatedPkgsAbsolutePaths || !result.updatedPkgsAbsolutePaths.length)) {
    const limitChild = pLimit(opts.childConcurrency)
    await Promise.all(
      R.props<ResolvedNode>(result.updatedPkgsAbsolutePaths, result.resolvedNodesMap)
        .map(resolvedNode => limitChild(async () => {
          try {
            await postInstall(resolvedNode.hardlinkedLocation, installLogger(resolvedNode.pkgId), {
              userAgent: opts.userAgent
            })
          } catch (err) {
            if (!installCtx.nonOptionalPackageIds.has(resolvedNode.pkgId)) {
              logger.warn({
                message: `Skipping failed optional dependency ${resolvedNode.pkgId}`,
                err,
              })
              return
            }
            throw err
          }
        })
      )
    )
  }

  if (installCtx.localPackages.length) {
    const linkOpts = Object.assign({}, opts, {skipInstall: true})
    await Promise.all(installCtx.localPackages.map(async localPackage => {
      await externalLink(localPackage.resolution.directory, opts.prefix, linkOpts)
      logStatus({
        status: 'installed',
        pkgId: localPackage.absolutePath,
      })
    }))
  }

  // waiting till the skipped packages are downloaded to the store
  await Promise.all(
    R.props<InstalledPackage>(Array.from(installCtx.skipped), installCtx.installs)
      // skipped packages might have not been reanalized on a repeat install
      // so lets just ignore those by excluding nulls
      .filter(Boolean)
      .map(pkg => pkg.fetchingFiles)
  )

  // waiting till integrities are saved
  await Promise.all(R.values(installCtx.installs).map(installed => installed.calculatingIntegrity))

  summaryLogger.info(undefined)
}

function buildTree (
  ctx: InstallContext,
  parentNodeId: string,
  parentPkgId: string,
  depth: number,
  installable: boolean
) {
  return ctx.installs[parentPkgId].children$
  .filter(childPkgId => !parentNodeId.includes(`:${parentPkgId}:${childPkgId}:`))
  .map(childPkgId => {
    const childNodeId = `${parentNodeId}${childPkgId}:`
    installable = installable && !ctx.skipped.has(childPkgId)
      ctx.tree[childNodeId] = {
        isCircular: parentNodeId.includes(`:${childPkgId}:`),
        nodeId: childNodeId,
        pkg: ctx.installs[childPkgId],
        children$: buildTree(ctx, childNodeId, childPkgId, depth + 1, installable),
        depth,
        installable,
      }
      return childNodeId
    })
}

function getTopParents (
  pkgNames: string[],
  modules: string
): Rx.Observable<{name: string, version: string}> {
  return Rx.Observable.from(pkgNames)
    .map(pkgName => path.join(modules, pkgName))
    .mergeMap(pkgPath => Rx.Observable.fromPromise(safeReadPkgFromDir(pkgPath)))
    .filter(Boolean)
    .map((pkg: Package) => ({
      name: pkg.name,
      version: pkg.version,
    }))
}

function getSaveSpec(spec: PackageSpec, version: string, saveExact: boolean) {
  switch (spec.type) {
    case 'version':
    case 'range':
    case 'tag':
      return `${saveExact ? '' : '^'}${version}`
    default:
      return spec.saveSpec
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
    const err = new Error(`Running event ${scriptName} failed with status ${result.status}`)
    err['code'] = 'ELIFECYCLE'
    throw err
  }
}

function installLogger (pkgId: string) {
  return (stream: string, line: string) => {
    const logLevel = stream === 'stderr' ? 'error' : 'info'
    lifecycleLogger[logLevel]({pkgId, line})
  }
}
