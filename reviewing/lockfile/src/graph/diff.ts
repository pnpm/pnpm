import { Graph, NodeKind, PackageNode } from './types'

function isSameNode (target: PackageNode, current: PackageNode): boolean {
  if (target.kind !== current.kind) {
    return false
  } else if (target.kind === NodeKind.Importer) {
    return target.key === current.key
  } else if (target.key === current.key) {
    return true
  } else {
    return target.name === current.name &&
      target.version === current.version &&
      target.peersSuffix === current.peersSuffix
  }
}

function diffImporters (target: string[], current: string[]): Diff<string> {
  const added = current.filter(key => !target.includes(key))
  const deleted = target.filter(key => !current.includes(key))
  return {
    added,
    deleted,
  }
}

interface Diff<T> {
  added: T[]
  deleted: T[]
}

interface DiffGraph {
  importers: Diff<string>
  packages: Diff<string>
}

export function diffGraph (targetGraph: Graph, currentGraph: Graph): DiffGraph {
  function diffPackages (target: string[], current: string[]): Diff<string> {
    const added = current.filter(cur => {
      const currentNode = currentGraph.getPackage(cur)!
      return target.findIndex(tar => isSameNode(targetGraph.getPackage(tar)!, currentNode)) === -1
    })
    const deleted = target.filter(tar => {
      const targetNode = targetGraph.getPackage(tar)!
      return current.findIndex(cur => isSameNode(targetNode, currentGraph.getPackage(cur)!)) === -1
    })
    return {
      added,
      deleted,
    }
  }

  return {
    importers: diffImporters(targetGraph.importerIds, currentGraph.importerIds),
    packages: diffPackages(targetGraph.packageKeys, currentGraph.packageKeys),
  }
}
