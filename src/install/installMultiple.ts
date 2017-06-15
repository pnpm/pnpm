import path = require('path')
import logger from 'pnpm-logger'
import R = require('ramda')
import getNpmTarballUrl from 'get-npm-tarball-url'
import exists = require('path-exists')
import fetch, {FetchedPackage} from './fetch'
import {InstallContext, InstalledPackages, PackageContentInfo} from '../api/install'
import {Dependencies} from '../types'
import memoize from '../memoize'
import {Package} from '../types'
import logStatus from '../logging/logInstallStatus'
import fs = require('mz/fs')
import {Got} from '../network/got'
import {
  getPkgId,
  getPkgShortId,
} from '../fs/shrinkwrap'
import {
  DependencyShrinkwrap,
  ResolvedDependencies,
} from 'pnpm-lockfile'
import {Resolution, PackageSpec, PackageMeta} from '../resolve'
import depsToSpecs from '../depsToSpecs'
import getIsInstallable from './getIsInstallable'
import pkgIdToFilename from '../fs/pkgIdToFilename'

export type PkgAddress = {
  nodeId: string,
  pkgId: string,
}

export type InstalledPackage = {
  id: string,
  resolution: Resolution,
  srcPath?: string,
  dev: boolean,
  optional: boolean,
  fetchingFiles: Promise<PackageContentInfo>,
  calculatingIntegrity: Promise<void>,
  path: string,
  specRaw: string,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  optionalDependencies: Set<string>,
  hasBundledDependencies: boolean,
  localLocation: string,
}

export default async function installMultiple (
  ctx: InstallContext,
  specs: PackageSpec[],
  options: {
    force: boolean,
    prefix: string,
    referencedFrom: string,
    storePath: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
  }
): Promise<PkgAddress[]> {
  const resolvedDependencies = options.resolvedDependencies || {}
  const pkgAddresses = <PkgAddress[]>(
    await Promise.all(
      specs
        .map(async (spec: PackageSpec) => {
          const reference = resolvedDependencies[spec.name]
          const pkgShortId = reference && getPkgShortId(reference, spec.name)
          const dependencyShrinkwrap = pkgShortId && ctx.shrinkwrap.packages[pkgShortId]
          const pkgId = dependencyShrinkwrap && dependencyShrinkwrap.id ||
            reference && getPkgId(reference, spec.name, ctx.shrinkwrap.registry)
          const shrinkwrapResolution: Resolution | undefined = pkgShortId && dependencyShrinkwrap
            ? dependencyShrToResolution(pkgShortId, dependencyShrinkwrap, options.registry)
            : undefined
          return await install(spec, ctx, Object.assign({}, options, {
            pkgId,
            resolvedDependencies: dependencyShrinkwrap &&
              <ResolvedDependencies>Object.assign({}, dependencyShrinkwrap.dependencies, dependencyShrinkwrap.optionalDependencies) || {},
            shrinkwrapResolution,
          }))
        })
    )
  )
  .filter(Boolean)

  return pkgAddresses
}

function dependencyShrToResolution (
  pkgShortId: string,
  depShr: DependencyShrinkwrap,
  registry: string
): Resolution {
  if (!depShr.resolution['type'] && !depShr.resolution['tarball']) {
    return Object.assign({}, depShr.resolution, {
      tarball: getTarball()
    })
  }
  return depShr.resolution as Resolution

  function getTarball () {
    const parts = pkgShortId.split('/')
    if (parts[1][0] === '@') {
      return getNpmTarballUrl(`${parts[1]}/${parts[2]}`, parts[3], {registry})
    }
    return getNpmTarballUrl(parts[1], parts[2], {registry})
  }
}

async function install (
  spec: PackageSpec,
  ctx: InstallContext,
  options: {
    force: boolean,
    prefix: string,
    referencedFrom: string,
    storePath: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[], // TODO: remove. Currently used only for logging
    pkgId?: string,
    parentNodeId: string,
    currentDepth: number,
    shrinkwrapResolution?: Resolution,
    resolvedDependencies: ResolvedDependencies,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    offline: boolean,
    parentIsInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
  }
): Promise<PkgAddress | null> {
  const keypath = options.keypath || []
  const proceed = keypath.length <= options.depth
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  if (!proceed && options.pkgId && await exists(path.join(options.nodeModules, `.${options.pkgId}`))) {
    return null
  }

  const registry = normalizeRegistry(spec.scope && options.rawNpmConfig[`${spec.scope}:registry`] || options.registry)

  const dependentId = keypath[keypath.length - 1]
  const loggedPkg = {
    rawSpec: spec.rawSpec,
    name: spec.name,
    dependentId,
  }
  logStatus({
    status: 'installing',
    pkg: loggedPkg,
  })

  const fetchedPkg = await fetch(spec, {
    loggedPkg,
    update: options.update,
    fetchingLocker: ctx.fetchingLocker,
    registry,
    prefix: options.prefix,
    storePath: options.storePath,
    metaCache: options.metaCache,
    got: options.got,
    shrinkwrapResolution: options.shrinkwrapResolution,
    pkgId: options.pkgId,
    offline: options.offline,
  })

  if (options.parentNodeId.indexOf(`:${dependentId}:${fetchedPkg.id}:`) !== -1) {
    return null
  }

  const pkg = await fetchedPkg.fetchingPkg
  logStatus({status: 'downloaded_manifest', pkgId: fetchedPkg.id, pkgVersion: pkg.version})

  const currentIsInstallable = (
      options.force ||
      await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, {
        optional: spec.optional,
        engineStrict: options.engineStrict,
        nodeVersion: options.nodeVersion,
      })
    )
  const installable = parentIsInstallable && currentIsInstallable

  // using colon as it will never be used inside a package ID
  const nodeId = `${options.parentNodeId}${fetchedPkg.id}:`

  if (installable) {
    ctx.skipped.delete(fetchedPkg.id)
  }
  if (!ctx.installs[fetchedPkg.id]) {
    if (!installable) {
      // optional dependencies are resolved for consistent shrinkwrap.yaml files
      // but installed only on machines that are supported by the package
      ctx.skipped.add(fetchedPkg.id)
    }

    ctx.installs[fetchedPkg.id] = {
      id: fetchedPkg.id,
      resolution: fetchedPkg.resolution,
      srcPath: fetchedPkg.srcPath,
      optional: spec.optional,
      name: pkg.name,
      version: pkg.version,
      dev: spec.dev,
      fetchingFiles: fetchedPkg.fetchingFiles,
      calculatingIntegrity: fetchedPkg.calculatingIntegrity,
      path: fetchedPkg.path,
      specRaw: spec.raw,
      peerDependencies: pkg.peerDependencies || {},
      optionalDependencies: new Set(R.keys(pkg.optionalDependencies)),
      hasBundledDependencies: !!(pkg.bundledDependencies || pkg.bundleDependencies),
      localLocation: path.join(options.nodeModules, `.${pkgIdToFilename(fetchedPkg.id)}`),
    }
    const children = await installDependencies(
      pkg,
      spec,
      fetchedPkg.id,
      ctx,
      Object.assign({}, options, {
        referencedFrom: fetchedPkg.srcPath,
        parentIsInstallable: installable,
        currentDepth: options.currentDepth + 1,
        parentNodeId: nodeId,
      })
    )
    ctx.childrenIdsByParentId[fetchedPkg.id] = children.map(child => child.pkgId)
    ctx.tree[nodeId] = {
      nodeId,
      pkg: ctx.installs[fetchedPkg.id],
      children: children.map(child => child.nodeId),
      depth: options.currentDepth,
      installable,
    }
  } else {
    ctx.installs[fetchedPkg.id].dev = ctx.installs[fetchedPkg.id].dev && spec.dev
    ctx.installs[fetchedPkg.id].optional = ctx.installs[fetchedPkg.id].optional && spec.optional

    ctx.nodesToBuild.push({
      nodeId,
      pkg: ctx.installs[fetchedPkg.id],
      depth: options.currentDepth,
      installable,
    })
  }
  // we need this for saving to package.json
  if (options.currentDepth === 0) {
    ctx.installs[fetchedPkg.id].specRaw = spec.raw
  }

  logStatus({status: 'dependencies_installed', pkgId: fetchedPkg.id})

  return {
    nodeId,
    pkgId: fetchedPkg.id,
  }
}

function normalizeRegistry (registry: string) {
  if (registry.endsWith('/')) return registry
  return `${registry}/`
}

async function installDependencies (
  pkg: Package,
  parentSpec: PackageSpec,
  pkgId: string,
  ctx: InstallContext,
  opts: {
    force: boolean,
    prefix: string,
    referencedFrom: string,
    storePath: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
  }
): Promise<PkgAddress[]> {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: opts.keypath.concat([ pkgId ]),
  })

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  const deps = depsToSpecs(
    filterDeps(Object.assign({}, pkg.optionalDependencies, pkg.dependencies)),
    {
      where: opts.referencedFrom,
      devDependencies: pkg.devDependencies || {},
      optionalDependencies: pkg.optionalDependencies || {},
    }
  )

  return await installMultiple(ctx, deps, depsInstallOpts)
}

function getNotBundledDeps (bundledDeps: string[], deps: Dependencies) {
  return Object.keys(deps)
    .filter(depName => bundledDeps.indexOf(depName) === -1)
    .reduce((notBundledDeps, depName) => {
      notBundledDeps[depName] = deps[depName]
      return notBundledDeps
    }, {})
}
