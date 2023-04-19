import { Graph, GraphBuilderContext, ImporterNode, Importers, LockfileWalkerStep, NodeKind, PackageNode, Packages } from './types'

function resolve (base: string, relative: string): string {
  if (relative.startsWith('/')) {
    return relative
  }
  const _base = base.split('/')
  for (const current of relative.split('/')) {
    if (current === '.') {
      continue
    } else if (current === '..') {
      _base.pop()
    } else if (current.length > 0) {
      _base.push(current)
    }
  }
  return _base.join('/')
}

export function createBuilder<
  Importer,
  Package extends { dependencies?: Record<string, string>, optionalDependencies?: Record<string, string> },
  Lockfile extends { importers: Record<string, Importer>, packages?: Record<string, Package> }
> (ctx: GraphBuilderContext<Importer, Package, Lockfile>) {
  return function (rawLockfile: string): Graph {
    const lockfile = ctx.parse(rawLockfile)
    const importerIds = Object.keys(lockfile.importers)
    const packagesIds = Object.keys(lockfile.packages ?? {})
    const importers: Importers = new Map(
      importerIds.map(
        id => [
          id,
          {
            key: id,
            children: new Set(),
            parents: new Set(),
            missing: new Set(),
            links: new Set(),
            kind: 0,
          },
        ]
      )
    )

    const packages: Packages = new Map(
      packagesIds.map(
        id => [
          id,
          {
            key: id,
            children: new Set(),
            parents: new Set(),
            missing: new Set(),
            links: new Set(),
            kind: 1,
          },
        ]
      )
    )

    function step (nextDepPaths: string[]): LockfileWalkerStep {
      const result: LockfileWalkerStep = {
        dependencies: [],
        links: [],
        missing: [],
      }

      for (const depPath of nextDepPaths) {
        const packageSnapshot = lockfile.packages?.[depPath]
        if (packageSnapshot === null || typeof packageSnapshot === 'undefined') {
          if (depPath.startsWith('link:')) {
            result.links.push(depPath)
            continue
          }
          result.missing.push(depPath)
          continue
        }
        result.dependencies.push({
          depPath,
          next: () => step(ctx.nextPackages(packageSnapshot)),
        })
      }

      return result
    }

    function dfs (pre: string, step: LockfileWalkerStep) {
      const parent = packages.get(pre)
      if (!parent) {
        throw Error('unreachable')
      }
      step.links.forEach(link => {
        const resolved = resolve(pre, link.slice(5))
        if (resolved.startsWith('/')) {
          const child = packages.get(resolved)!
          parent.links.add(child)
          child.parents.add(parent)
        } else {
          const child = importers.get(resolved)!
          parent.links.add(child)
          child.parents.add(parent)
        }
      })
      if (step.links.length > 0) {
        throw Error('')
      }
      step.missing.forEach(missing => {
        parent.missing.add(missing)
      })
      step.dependencies.forEach(dep => {
        const child = packages.get(dep.depPath)!
        setPackageNodePath(child)
        if (!parent.children.has(child)) {
          parent.children.add(child)
          child.parents.add(parent)
          dfs(dep.depPath, dep.next())
        }
      })
    }

    importerIds.forEach(importId => {
      const projectSnapshot = lockfile.importers[importId]
      const entryNodes = ctx.entryNodes(projectSnapshot)
      const importer = importers.get(importId)!
      const firstStep = step(entryNodes)

      firstStep.links.forEach(link => {
        const resolved = resolve(importId, link.slice(5))
        if (resolved.startsWith('/')) {
          const child = packages.get(resolved)!
          importer.links.add(child)
          child.parents.add(importer)
        } else {
          const child = importers.get(resolved)!
          importer.links.add(child)
          child.parents.add(importer)
        }
      })
      firstStep.missing.forEach(missing => {
        importer.missing.add(missing)
      })
      firstStep.dependencies.forEach(dep => {
        const child = packages.get(dep.depPath)!
        setPackageNodePath(child)
        importer.children.add(child)
        child.parents.add(importer)
        const next = dep.next()
        dfs(dep.depPath, next)
      })
    })

    function setPackageNodePath (node: PackageNode) {
      const { host, name, version, peersSuffix } = ctx.parseDependencyPath(node.key)
      if (host) {
        node.host = host
      }
      if (name) {
        node.name = name
      }
      if (version) {
        node.version = version
      }
      if (peersSuffix) {
        node.peersSuffix = peersSuffix
      }
    }

    return build(importers, packages)
  }
}

function build (importers: Importers, packages: Packages): Graph {
  const importerIds = Array.from(importers.keys())
  const packageKeys = Array.from(packages.keys())
  return {
    importerIds,
    packageKeys,
    getImporter (importerId) {
      return importers.get(importerId)
    },
    getPackage (packageKey) {
      return packages.get(packageKey)
    },
    whoImportThisPackage (packageKey) {
      return whoImportThisPackage(packages, packageKey)
    },
  }
}

function whoImportThisPackage (packages: Packages, packageKey: string): string[][] | undefined {
  const node = packages.get(packageKey)
  if (!node) {
    return undefined
  }
  const paths: string[][] = []
  function dfs (prefix: Set<string>, node: PackageNode | ImporterNode) {
    if (prefix.has(node.key)) {
      paths.push([node.key + '|circle', ...prefix])
      return
    }
    node.parents.forEach(next => {
      if (next.kind === NodeKind.Importer) {
        paths.push([next.key, node.key, ...prefix])
      } else {
        dfs(new Set([node.key, ...prefix]), next)
      }
    })
  }
  dfs(new Set(), node)
  return paths
}
