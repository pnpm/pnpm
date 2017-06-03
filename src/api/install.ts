import path = require('path')
import RegClient = require('npm-registry-client')
import logger from 'pnpm-logger'
import pLimit = require('p-limit')
import npa = require('npm-package-arg')
import pFilter = require('p-filter')
import R = require('ramda')
import safeIsInnerLink from '../safeIsInnerLink'
import {fromDir as safeReadPkgFromDir} from '../fs/safeReadPkg'
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
  Shrinkwrap,
  ResolvedDependencies,
  pkgIdToRef,
} from '../fs/shrinkwrap'
import {save as saveModules} from '../fs/modulesController'
import mkdirp = require('mkdirp-promise')
import createMemoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {PackageSpec} from '../resolve'
import {DependencyTreeNode} from '../link/resolvePeers'
import depsToSpecs, {similarDepsToSpecs} from '../depsToSpecs'

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type TreeNode = {
  nodeId: string,
  children: string[], // Node IDs of children
  pkg: InstalledPackage,
  depth: number,
  installable: boolean,
}

export type TreeNodeMap = {
  [nodeId: string]: TreeNode,
}

export type InstallContext = {
  installs: InstalledPackages,
  childrenIdsByParentId: {[parentId: string]: string[]},
  nodesToBuild: {
    nodeId: string,
    pkg: InstalledPackage,
    depth: number,
    installable: boolean,
    parentNodeId: string,
  }[],
  shrinkwrap: Shrinkwrap,
  fetchingLocker: MemoizedFunc<Boolean>,
  // the IDs of packages that are not installable
  skipped: Set<string>,
  tree: {[nodeId: string]: TreeNode},
}

export async function install (maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  return lock(opts.prefix, async () => {
    const installType = 'general'
    const ctx = await getContext(opts, installType)
    const installCtx = await createInstallCmd(opts, ctx.shrinkwrap, ctx.skipped)

    if (!ctx.pkg) throw new Error('No package.json found')

    const specs = specsToInstallFromPackage(ctx.pkg, {
      prefix: opts.prefix,
    })

    specs.forEach(spec => {
      if (ctx.shrinkwrap.specifiers && ctx.shrinkwrap.specifiers[spec.name] !== spec.rawSpec) {
        delete ctx.shrinkwrap.dependencies[spec.name]
      }
    })

    if (opts.lock === false) {
      return run()
    }

    return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

    async function run () {
      const scripts = !opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts || {}
      if (scripts['preinstall']) {
        npmRun('preinstall', ctx.root, opts.userAgent)
      }

      await installInContext(installType, specs, [], ctx, installCtx, opts)

      if (scripts['postinstall']) {
        npmRun('postinstall', ctx.root, opts.userAgent)
      }
      if (scripts['prepublish']) {
        npmRun('prepublish', ctx.root, opts.userAgent)
      }
    }
  }, {stale: opts.lockStaleDuration})
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

function depsFromPackage (pkg: Package): Dependencies {
  return Object.assign(
    {},
    pkg.devDependencies,
    pkg.optionalDependencies,
    pkg.dependencies
  ) as Dependencies
}

/**
 * Perform installation.
 *
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { silent: true })
 */
export async function installPkgs (fuzzyDeps: string[] | Dependencies, maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  return lock(opts.prefix, async () => {
    let packagesToInstall = Array.isArray(fuzzyDeps)
      ? argsToSpecs(fuzzyDeps, {
        defaultTag: opts.tag,
        where: opts.prefix,
        dev: opts.saveDev,
        optional: opts.saveOptional,
      })
      : similarDepsToSpecs(fuzzyDeps, {
        where: opts.prefix,
        dev: opts.saveDev,
        optional: opts.saveOptional,
      })

    if (!Object.keys(packagesToInstall).length) {
      throw new Error('At least one package has to be installed')
    }
    const installType = 'named'
    const ctx = await getContext(opts, installType)
    const installCtx = await createInstallCmd(opts, ctx.shrinkwrap, ctx.skipped)

    packagesToInstall.forEach(spec => {
      delete ctx.shrinkwrap.dependencies[spec.name]
    })

    if (opts.lock === false) {
      return run()
    }

    return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

    function run () {
      return installInContext(
        installType,
        packagesToInstall,
        packagesToInstall.map(spec => spec.name),
        ctx,
        installCtx,
        opts)
    }
  }, {stale: opts.lockStaleDuration})
}

function argsToSpecs (
  args: string[],
  opts: {
    defaultTag: string,
    where: string,
    dev: boolean,
    optional: boolean,
  }
): PackageSpec[] {
  return args
    .map(arg => npa(arg, opts.where))
    .map(spec => {
      if (spec.type === 'tag' && !spec.rawSpec) {
        spec.fetchSpec = opts.defaultTag
      }
      spec.dev = opts.dev
      spec.optional = opts.optional
      return spec
    })
}

async function installInContext (
  installType: string,
  packagesToInstall: PackageSpec[],
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
    resolvedDependencies: ctx.shrinkwrap.dependencies,
    offline: opts.offline,
    rawNpmConfig: opts.rawNpmConfig,
    nodeModules: nodeModulesPath,
    update: opts.update,
    keypath: [],
    referencedFrom: opts.prefix,
    prefix: opts.prefix,
    parentNodeId: ':/:',
    currentDepth: 0,
  }
  const nonLinkedPkgs = await pFilter(packagesToInstall, (spec: PackageSpec) => !spec.name || safeIsInnerLink(nodeModulesPath, spec.name))
  const rootPkgs = await installMultiple(
    installCtx,
    nonLinkedPkgs,
    installOpts
  )
  const rootNodeIds = rootPkgs.map(pkg => pkg.nodeId)
  installCtx.nodesToBuild.forEach(nodeToBuild => {
    installCtx.tree[nodeToBuild.nodeId] = {
      nodeId: nodeToBuild.nodeId,
      pkg: nodeToBuild.pkg,
      children: buildTree(installCtx, nodeToBuild.parentNodeId, nodeToBuild.pkg.id,
        installCtx.childrenIdsByParentId[nodeToBuild.pkg.id], nodeToBuild.depth + 1, nodeToBuild.installable),
      depth: nodeToBuild.depth,
      installable: nodeToBuild.installable,
    }
  })
  const pkgs: InstalledPackage[] = R.props<TreeNode>(rootNodeIds, installCtx.tree).map(node => node.pkg)

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
          name: dep.name,
          saveSpec: getSaveSpec(spec, dep, opts.saveExact)
        }
      }).filter(Boolean),
      saveType
    )
  }

  if (newPkg) {
    ctx.shrinkwrap.dependencies = ctx.shrinkwrap.dependencies || {}
    ctx.shrinkwrap.specifiers = ctx.shrinkwrap.specifiers || {}

    const deps = newPkg.dependencies || {}
    const devDeps = newPkg.devDependencies || {}
    const optionalDeps = newPkg.optionalDependencies || {}

    const getSpecFromPkg = (depName: string) => deps[depName] || devDeps[depName] || optionalDeps[depName]

    pkgs.forEach(dep => {
      ctx.shrinkwrap.dependencies[dep.name] = pkgIdToRef(dep.id, dep.name, dep.resolution, ctx.shrinkwrap.registry)
      ctx.shrinkwrap.specifiers[dep.name] = getSpecFromPkg(dep.name)
    })
  }

  const result = await linkPackages(pkgs, rootNodeIds, installCtx.tree, {
    force: opts.force,
    global: opts.global,
    baseNodeModules: nodeModulesPath,
    bin: opts.bin,
    topParents: ctx.pkg
      ? await getTopParents(
          R.difference(R.keys(depsFromPackage(ctx.pkg)), newPkgs), nodeModulesPath)
      : [],
    shrinkwrap: ctx.shrinkwrap,
    production: opts.production,
    root: ctx.root,
    privateShrinkwrap: ctx.privateShrinkwrap,
    storePath: ctx.storePath,
    skipped: ctx.skipped,
    pkg: newPkg || ctx.pkg,
  })

  await saveShrinkwrap(ctx.root, result.shrinkwrap)
  await saveModules(path.join(ctx.root, 'node_modules'), {
    packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
    storePath: ctx.storePath,
    skipped: Array.from(installCtx.skipped),
  })

  // postinstall hooks
  if (!(opts.ignoreScripts || !result.newPkgResolvedIds || !result.newPkgResolvedIds.length)) {
    const limitChild = pLimit(opts.childConcurrency)
    const linkedPkgsMapValues = R.values(result.linkedPkgsMap)
    await Promise.all(
      R.props<DependencyTreeNode>(result.newPkgResolvedIds, result.linkedPkgsMap)
        .map(pkg => limitChild(async () => {
          try {
            await postInstall(pkg.hardlinkedLocation, installLogger(pkg.id), {
              userAgent: opts.userAgent
            })
          } catch (err) {
            if (installCtx.installs[pkg.id].optional) {
              logger.warn({
                message: `Skipping failed optional dependency ${pkg.id}`,
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
}

function buildTree (
  ctx: InstallContext,
  parentNodeId: string,
  parentId: string,
  childrenIds: string[],
  depth: number,
  installable: boolean
) {
  const childrenNodeIds = []
  for (const childId of childrenIds) {
    if (parentNodeId.indexOf(`:${parentId}:${childId}:`) !== -1) {
      continue
    }
    const childNodeId = `${parentNodeId}${childId}:`
    childrenNodeIds.push(childNodeId)
    ctx.tree[childNodeId] = {
      nodeId: childNodeId,
      pkg: ctx.installs[childId],
      children: buildTree(ctx, childNodeId, childId, ctx.childrenIdsByParentId[childId], depth + 1, installable),
      depth,
      installable,
    }
  }
  return childrenNodeIds
}

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map(pkgName => path.join(modules, pkgName)).map(safeReadPkgFromDir)
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
      return `${saveExact ? '' : '^'}${pkg.version}`
    default:
      return spec.saveSpec
  }
}

async function createInstallCmd (opts: StrictPnpmOptions, shrinkwrap: Shrinkwrap, skipped: Set<string>): Promise<InstallContext> {
  return {
    installs: {},
    childrenIdsByParentId: {},
    nodesToBuild: [],
    shrinkwrap,
    fetchingLocker: createMemoize<boolean>(opts.fetchingConcurrency),
    skipped,
    tree: {},
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
