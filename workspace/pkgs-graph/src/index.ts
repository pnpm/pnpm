import path from 'path'
import npa from '@pnpm/npm-package-arg'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import mapValues from 'ramda/src/map'

export interface Manifest {
  name?: string
  version?: string
  dependencies?: {
    [name: string]: string
  }
  devDependencies?: {
    [name: string]: string
  }
  optionalDependencies?: {
    [name: string]: string
  }
}

export interface Package {
  manifest: Manifest
  dir: string
}

export interface PackageNode<T> {
  package: Package & T
  dependencies: string[]
}

export function createPkgGraph<T> (pkgs: Array<Package & T>, opts?: {
  ignoreDevDeps?: boolean
  linkWorkspacePackages?: boolean
}): {
    graph: Record<string, PackageNode<T>>
    unmatched: Array<{ pkgName: string, range: string }>
  } {
  const pkgMap = createPkgMap(pkgs)
  const pkgMapValues = Object.values(pkgMap)
  const unmatched: Array<{ pkgName: string, range: string }> = []
  const graph = mapValues((pkg) => ({
    dependencies: createNode(pkg),
    package: pkg,
  }), pkgMap) as Record<string, PackageNode<T>>
  return { graph, unmatched }

  function createNode (pkg: Package): string[] {
    const dependencies = {
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
            rawSpec = rawSpec.slice(10)
            if (rawSpec === '^' || rawSpec === '~') {
              rawSpec = '*'
            }
          }
          spec = npa.resolve(depName, rawSpec, pkg.dir)
        } catch (err: any) { // eslint-disable-line
          return ''
        }

        if (spec.type === 'directory') {
          const matchedPkg = pkgMapValues.find(pkg => path.relative(pkg.dir, spec.fetchSpec) === '')
          if (matchedPkg == null) {
            return ''
          }
          return matchedPkg.dir
        }

        if (spec.type !== 'version' && spec.type !== 'range') return ''

        const pkgs = pkgMapValues.filter(pkg => pkg.manifest.name === depName)
        if (pkgs.length === 0) return ''
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
          return matchedPkg!.dir
        }
        if (versions.includes(rawSpec)) {
          const matchedPkg = pkgs.find(pkg => pkg.manifest.name === depName && pkg.manifest.version === rawSpec)
          return matchedPkg!.dir
        }
        const matched = resolveWorkspaceRange(rawSpec, versions)
        if (!matched) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
        }
        const matchedPkg = pkgs.find(pkg => pkg.manifest.name === depName && pkg.manifest.version === matched)
        return matchedPkg!.dir
      })
      .filter(Boolean)
  }
}

function createPkgMap (pkgs: Package[]): Record<string, Package> {
  const pkgMap: Record<string, Package> = {}
  for (const pkg of pkgs) {
    pkgMap[pkg.dir] = pkg
  }
  return pkgMap
}
