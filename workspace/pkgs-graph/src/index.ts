import path from 'path'
import npa from '@pnpm/npm-package-arg'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import { parsePref, workspacePrefToNpm } from '@pnpm/npm-resolver'
import mapValues from 'ramda/src/map'

export interface Snapshot {
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  optionalDependencies?: Record<string, string>
}

export type Package =
  | { type: 'package', lockfileDir: string, name?: string, version?: string, id: string, snapshot: Snapshot }
  | { type: 'project', lockfileDir: string, name?: string, version?: string, resolvedDir: string, snapshot: Snapshot }

export type GraphKey = `${'package'}:${string}:${string}` | `${'project'}:${string}`

export interface PackageNode<T> {
  package: Package & T
  dependencies: GraphKey[]
}

export function createPkgGraph<T> (pkgs: Array<Package & T>, opts?: {
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
}): {
    graph: Record<GraphKey, PackageNode<T>>
    unmatched: Array<{ pkgName: string, range: string }>
  } {
  const pkgMap = createPkgMap(pkgs)
  const pkgMapValues = Object.values(pkgMap)
  let pkgMapByManifestName: Record<string, Package[] | undefined> | undefined
  let pkgMapByGraphKey: Record<GraphKey, Package | undefined> | undefined
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const graph = mapValues((pkg) => ({
    dependencies: createNode(pkg),
    package: pkg,
  }), pkgMap) as Record<GraphKey, PackageNode<T>>
  return { graph, unmatched }

  function createNode (pkg: Package): GraphKey[] {
    if (pkg.type === 'package') {
      const dependencies = {
        ...pkg.snapshot.optionalDependencies,
        ...pkg.snapshot.dependencies,
      }
      return Object.entries(dependencies)
        .map(([depName, rawSpec]) => {
          pkgMapByManifestName ??= getPkgMapByManifestName(pkgMapValues)
          const pkgs = pkgMapByManifestName[depName]
          if (!pkgs || pkgs.length === 0) return null
          const versions = pkgs.map(pkg => pkg.version).filter(Boolean) as string[]

          if (versions.includes(rawSpec)) {
            const matchedPkg = pkgs.find(pkg => pkg.name === depName && pkg.version === rawSpec)
            return createGraphKey(matchedPkg!)
          }

          const matched = resolveWorkspaceRange(rawSpec, versions)
          if (!matched) {
            unmatched.push({ pkgName: depName, range: rawSpec })
            return null
          }
          const matchedPkg = pkgs.find(pkg => pkg.name === depName && pkg.version === matched)
          return createGraphKey(matchedPkg!)
        })
        .filter(isNotNull)
    }

    if (pkg.type !== 'project') {
      throw new Error(`Package item of type ${(pkg as Package).type} was unaccounted for`)
    }

    const dependencies = {
      ...(!opts?.ignoreDevDeps && pkg.snapshot.devDependencies),
      ...pkg.snapshot.optionalDependencies,
      ...pkg.snapshot.dependencies,
    }

    return Object.entries(dependencies)
      .map(([depName, rawSpec]): GraphKey | null => {
        let spec!: { fetchSpec: string, type: string }
        const isWorkspaceSpec = rawSpec.startsWith('workspace:')
        try {
          if (isWorkspaceSpec) {
            const { fetchSpec, name } = parsePref(workspacePrefToNpm(rawSpec), depName, 'latest', '')!
            rawSpec = fetchSpec
            depName = name
          }
          spec = npa.resolve(depName, rawSpec, pkg.resolvedDir)
        } catch {
          return null
        }

        if (spec.type === 'directory') {
          pkgMapByGraphKey ??= getPkgMapByGraphKey(pkgMapValues)
          const resolvedPath = path.resolve(pkg.resolvedDir, spec.fetchSpec)
          const found = pkgMapByGraphKey[`project:${resolvedPath}`]
          if (found) {
            return createGraphKey(found)
          }

          // Slow path; only needed when there are case mismatches on case-insensitive filesystems.
          const matchedPkg = pkgMapValues.find(pkg => pkg.type === 'project' && path.relative(pkg.resolvedDir, spec.fetchSpec) === '')
          if (matchedPkg == null) {
            return null
          }
          pkgMapByGraphKey[`project:${resolvedPath}`] = matchedPkg
          return createGraphKey(matchedPkg)
        }

        if (spec.type !== 'version' && spec.type !== 'range') return null

        pkgMapByManifestName ??= getPkgMapByManifestName(pkgMapValues)
        const pkgs = pkgMapByManifestName[depName]
        if (!pkgs || pkgs.length === 0) return null
        const versions = pkgs.map(pkg => pkg.version).filter(Boolean) as string[]

        // explicitly check if false, backwards-compatibility (can be undefined)
        const strictWorkspaceMatching = opts?.linkWorkspacePackages === false && !isWorkspaceSpec
        if (strictWorkspaceMatching) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return null
        }
        if (isWorkspaceSpec && versions.length === 0) {
          const matchedPkg = pkgs.find(pkg => pkg.name === depName)
          return createGraphKey(matchedPkg!)
        }
        if (versions.includes(rawSpec)) {
          const matchedPkg = pkgs.find(pkg => pkg.name === depName && pkg.version === rawSpec)
          return createGraphKey(matchedPkg!)
        }
        const matched = resolveWorkspaceRange(rawSpec, versions)
        if (!matched) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return null
        }
        const matchedPkg = pkgs.find(pkg => pkg.name === depName && pkg.version === matched)
        return createGraphKey(matchedPkg!)
      })
      .filter(isNotNull)
  }
}

function createPkgMap (pkgs: Package[]): Record<GraphKey, Package> {
  const pkgMap: Record<string, Package> = {}
  for (const pkg of pkgs) {
    pkgMap[createGraphKey(pkg)] = pkg
  }
  return pkgMap
}

function getPkgMapByManifestName (pkgMapValues: Package[]) {
  const pkgMapByManifestName: Record<string, Package[] | undefined> = {}
  for (const pkg of pkgMapValues) {
    if (pkg.name) {
      (pkgMapByManifestName[pkg.name] ??= []).push(pkg)
    }
  }
  return pkgMapByManifestName
}

function getPkgMapByGraphKey (pkgMapValues: Package[]) {
  const pkgMapByDir: Record<string, Package | undefined> = {}
  for (const pkg of pkgMapValues) {
    pkgMapByDir[createGraphKey(pkg)] = pkg
  }
  return pkgMapByDir
}

function createGraphKey (pkg: Package): GraphKey {
  if (pkg.type === 'package') {
    return `package:${pkg.lockfileDir}:${pkg.id}`
  }
  if (pkg.type === 'project') {
    return `project:${pkg.resolvedDir}`
  }
  throw new Error(`Package item of type ${(pkg as Package).type} was unaccounted for`)
}

function isNotNull<T> (value?: T | null): value is T {
  return value !== null && value !== undefined
}
