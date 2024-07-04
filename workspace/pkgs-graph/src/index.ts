import path from 'path'
import npa from '@pnpm/npm-package-arg'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import { parsePref, workspacePrefToNpm } from '@pnpm/npm-resolver'
import { type ProjectRootDir, type BaseManifest } from '@pnpm/types'
import mapValues from 'ramda/src/map'

export interface Package {
  manifest: BaseManifest
  rootDir: ProjectRootDir
}

export interface PackageNode<Pkg extends Package> {
  package: Pkg
  dependencies: ProjectRootDir[]
}

export function createPkgGraph<Pkg extends Package> (pkgs: Pkg[], opts?: {
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
}): {
    graph: Record<ProjectRootDir, PackageNode<Pkg>>
    unmatched: Array<{ pkgName: string, range: string }>
  } {
  const pkgMap = createPkgMap(pkgs)
  const pkgMapValues = Object.values(pkgMap)
  let pkgMapByManifestName: Record<string, Package[] | undefined> | undefined
  let pkgMapByDir: Record<string, Package | undefined> | undefined
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const graph = mapValues((pkg) => ({
    dependencies: createNode(pkg),
    package: pkg,
  }), pkgMap) as Record<ProjectRootDir, PackageNode<Pkg>>
  return { graph, unmatched }

  function createNode (pkg: Package): string[] {
    const dependencies = {
      ...pkg.manifest.peerDependencies,
      ...(!opts?.ignoreDevDeps && pkg.manifest.devDependencies),
      ...pkg.manifest.optionalDependencies,
      ...pkg.manifest.dependencies,
    }

    return Object.entries(dependencies)
      .map(([depName, rawSpec]) => {
        let spec!: { fetchSpec: string, type: string }
        const isWorkspaceSpec = rawSpec.startsWith('workspace:')
        try {
          if (isWorkspaceSpec) {
            const { fetchSpec, name } = parsePref(workspacePrefToNpm(rawSpec), depName, 'latest', '')!
            rawSpec = fetchSpec
            depName = name
          }
          spec = npa.resolve(depName, rawSpec, pkg.rootDir)
        } catch {
          return ''
        }

        if (spec.type === 'directory') {
          pkgMapByDir ??= getPkgMapByDir(pkgMapValues)
          const resolvedPath = path.resolve(pkg.rootDir, spec.fetchSpec)
          const found = pkgMapByDir[resolvedPath]
          if (found) {
            return found.rootDir
          }

          // Slow path; only needed when there are case mismatches on case-insensitive filesystems.
          const matchedPkg = pkgMapValues.find(pkg => path.relative(pkg.rootDir, spec.fetchSpec) === '')
          if (matchedPkg == null) {
            return ''
          }
          pkgMapByDir[resolvedPath] = matchedPkg
          return matchedPkg.rootDir
        }

        if (spec.type !== 'version' && spec.type !== 'range') return ''

        pkgMapByManifestName ??= getPkgMapByManifestName(pkgMapValues)
        const pkgs = pkgMapByManifestName[depName]
        if (!pkgs || pkgs.length === 0) return ''
        const versions = pkgs.filter(({ manifest }) => manifest.version)
          .map(pkg => pkg.manifest.version) as string[]

        // explicitly check if false, backwards-compatibility (can be undefined)
        const strictWorkspaceMatching = opts?.linkWorkspacePackages === false && !isWorkspaceSpec
        if (strictWorkspaceMatching) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
        }
        if (isWorkspaceSpec && versions.length === 0) {
          const matchedPkg = pkgs.find(pkg => pkg.manifest.name === depName)
          return matchedPkg!.rootDir
        }
        if (versions.includes(rawSpec)) {
          const matchedPkg = pkgs.find(pkg => pkg.manifest.name === depName && pkg.manifest.version === rawSpec)
          return matchedPkg!.rootDir
        }
        const matched = resolveWorkspaceRange(rawSpec, versions)
        if (!matched) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
        }
        const matchedPkg = pkgs.find(pkg => pkg.manifest.name === depName && pkg.manifest.version === matched)
        return matchedPkg!.rootDir
      })
      .filter(Boolean)
  }
}

function createPkgMap (pkgs: Package[]): Record<ProjectRootDir, Package> {
  const pkgMap: Record<ProjectRootDir, Package> = {}
  for (const pkg of pkgs) {
    pkgMap[pkg.rootDir] = pkg
  }
  return pkgMap
}

function getPkgMapByManifestName (pkgMapValues: Package[]): Record<string, Package[] | undefined> {
  const pkgMapByManifestName: Record<string, Package[] | undefined> = {}
  for (const pkg of pkgMapValues) {
    if (pkg.manifest.name) {
      (pkgMapByManifestName[pkg.manifest.name] ??= []).push(pkg)
    }
  }
  return pkgMapByManifestName
}

function getPkgMapByDir (pkgMapValues: Package[]): Record<string, Package | undefined> {
  const pkgMapByDir: Record<string, Package | undefined> = {}
  for (const pkg of pkgMapValues) {
    pkgMapByDir[path.resolve(pkg.rootDir)] = pkg
  }
  return pkgMapByDir
}
