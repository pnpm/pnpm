import resolveWorkspaceRange from '@pnpm/resolve-workspace-range'
import path = require('path')
import npa = require('@zkochan/npm-package-arg')
import R = require('ramda')

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

export default function<T> (pkgs: Array<Package & T>, opts?: {
  linkWorkspacePackages?: boolean
}): {
    graph: {[id: string]: PackageNode<T>}
    unmatched: Array<{pkgName: string, range: string}>
  } {
  const pkgMap = createPkgMap(pkgs)
  const unmatched: Array<{pkgName: string, range: string}> = []
  const graph = Object.keys(pkgMap)
    .reduce((acc, pkgSpec) => {
      acc[pkgSpec] = {
        dependencies: createNode(pkgMap[pkgSpec]),
        package: pkgMap[pkgSpec],
      }
      return acc
    }, {})

  return { graph, unmatched }

  function createNode (pkg: Package): string[] {
    const dependencies = Object.assign({},
      pkg.manifest.devDependencies,
      pkg.manifest.optionalDependencies,
      pkg.manifest.dependencies)

    return Object.keys(dependencies)
      .map(depName => {
        let spec!: { fetchSpec: string, type: string }
        let rawSpec = dependencies[depName]
        const isWorkspaceSpec = rawSpec.startsWith('workspace:')
        try {
          if (isWorkspaceSpec) {
            rawSpec = rawSpec.substr(10)
          }
          spec = npa.resolve(depName, rawSpec, pkg.dir)
        } catch (err) {
          return ''
        }

        if (spec.type === 'directory') {
          const matchedPkg = R.values(pkgMap).find(pkg => path.relative(pkg.dir, spec.fetchSpec) === '')
          if (!matchedPkg) {
            return ''
          }
          return matchedPkg.dir
        }

        if (spec.type !== 'version' && spec.type !== 'range') return ''

        const pkgs = R.values(pkgMap).filter(pkg => pkg.manifest.name === depName)
        if (!pkgs.length) return ''
        const versions = pkgs.filter(({ manifest }) => manifest.version)
          .map(pkg => pkg.manifest.version) as string[]

        // explicitly check if false, backwards-compatibility (can be undefined)
        const strictWorkspaceMatching = opts?.linkWorkspacePackages === false && !isWorkspaceSpec
        if (strictWorkspaceMatching) {
          unmatched.push({ pkgName: depName, range: rawSpec })
          return ''
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

function createPkgMap (pkgs: Package[]): {
  [pkgId: string]: Package
} {
  const pkgMap = {}
  for (const pkg of pkgs) {
    pkgMap[pkg.dir] = pkg
  }
  return pkgMap
}
