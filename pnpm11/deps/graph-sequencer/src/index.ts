export type Graph<T> = Map<T, T[]>
export type Groups<T> = T[][]

export interface Options<T> {
  graph: Graph<T>
  groups: Groups<T>
}

export interface Result<T> {
  safe: boolean
  chunks: Groups<T>
  cycles: Groups<T>
}

/**
 * Performs topological sorting on a graph while supporting node restrictions.
 *
 * The nodes are interned to indices up front and each chunk is gathered from
 * the nodes whose degree a removal drops to zero, so a workspace-scale graph
 * (thousands of projects in thousands of chunks) sorts in O(V log V + E)
 * instead of scanning every node once per chunk.
 *
 * @param {Graph<T>}  graph - The graph represented as a Map where keys are nodes and values are their outgoing edges.
 * @param {T[]} includedNodes - An array of nodes that should be included in the sorting process. Other nodes will be ignored.
 * @returns {Result<T>} An object containing the result of the sorting, including safe, chunks, and cycles.
 */
export function graphSequencer<T> (graph: Graph<T>, includedNodes: T[] = [...graph.keys()]): Result<T> {
  // Included nodes are interned first, so an id below includedCount is an
  // included node and id order chunks the way includedNodes orders them.
  const indexOf = new Map<T, number>()
  const nodes: T[] = []
  function intern (node: T): number {
    let id = indexOf.get(node)
    if (id === undefined) {
      id = nodes.length
      indexOf.set(node, id)
      nodes.push(node)
    }
    return id
  }
  for (const node of includedNodes) {
    intern(node)
  }
  const includedCount = nodes.length
  for (const [from, edges] of graph.entries()) {
    intern(from)
    for (const to of edges) {
      intern(to)
    }
  }

  const adjacency: number[][] = nodes.map(() => [])
  const reverseGraph: number[][] = nodes.map(() => [])
  const outDegree: number[] = nodes.map(() => 0)
  for (const [from, edges] of graph.entries()) {
    const fromId = indexOf.get(from)!
    for (const to of edges) {
      const toId = indexOf.get(to)!
      adjacency[fromId].push(toId)
      if (fromId < includedCount && toId < includedCount) {
        outDegree[fromId]++
        reverseGraph[toId].push(fromId)
      }
    }
  }

  // A non-included node is born removed: chunks never contain it and the
  // cycle search does not walk through it.
  const removed: boolean[] = nodes.map((_, id) => id >= includedCount)

  const chunks: number[][] = []
  const cycles: number[][] = []
  let safe = true

  let remaining = includedCount
  // The ids whose degree is zero, i.e. the next chunk. Kept sorted so a
  // chunk lists its nodes in includedNodes order.
  let current: number[] = []
  for (let id = 0; id < includedCount; id++) {
    if (outDegree[id] === 0) {
      current.push(id)
    }
  }
  while (remaining > 0) {
    const next: number[] = []
    const removeNode = (id: number) => {
      removed[id] = true
      for (const parent of reverseGraph[id]) {
        if (outDegree[parent] > 0) {
          outDegree[parent]--
          if (outDegree[parent] === 0 && !removed[parent]) {
            next.push(parent)
          }
        }
      }
    }

    if (current.length === 0) {
      // Every remaining node keeps a dependency alive: cycles. Break them
      // the way the scan finds them, in includedNodes order.
      const cycleIds: number[] = []
      for (let id = 0; id < includedCount; id++) {
        if (removed[id]) {
          continue
        }
        const cycle = findCycle(id)
        if (cycle.length === 0) {
          continue
        }
        if (cycle.length > 1) {
          safe = false
        }
        for (const node of cycle) {
          removeNode(node)
        }
        cycleIds.push(...cycle)
        cycles.push(cycle)
      }
      remaining -= cycleIds.length
      chunks.push(cycleIds)
    } else {
      for (const id of current) {
        removeNode(id)
      }
      remaining -= current.length
      chunks.push(current)
    }
    // Breaking a cycle removes its members one by one, so an earlier
    // member's removal can drop a later member to degree zero right before
    // that member is removed too — filter those out of the zero-degree set
    // instead of re-chunking them.
    current = next.filter((id) => !removed[id]).sort((left, right) => left - right)
  }

  return {
    safe,
    chunks: chunks.map((chunk) => chunk.map((id) => nodes[id])),
    cycles: cycles.map((cycle) => cycle.map((id) => nodes[id])),
  }

  // The longest of the shortest cycles running from startId back to itself
  // through nodes not yet removed, or empty when there is none.
  function findCycle (startId: number): number[] {
    const queue: Array<[number, number[]]> = [[startId, [startId]]]
    const cycleVisited = new Set<number>()
    const foundCycles: number[][] = []

    while (queue.length) {
      const [id, cycle] = queue.shift()!
      for (const to of adjacency[id]) {
        if (to === startId) {
          cycleVisited.add(to)
          foundCycles.push([...cycle])
          continue
        }
        if (removed[to] || cycleVisited.has(to)) {
          continue
        }
        cycleVisited.add(to)
        queue.push([to, [...cycle, to]])
      }
    }

    if (foundCycles.length === 0) {
      return []
    }
    foundCycles.sort((a, b) => b.length - a.length)
    return foundCycles[0]
  }
}
