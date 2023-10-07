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
 * @param {Graph<T>}  graph - The graph represented as a Map where keys are nodes and values are their outgoing edges.
 * @param {T[]} includedNodes - An array of nodes that should be included in the sorting process. Other nodes will be ignored.
 * @returns {Result<T>} An object containing the result of the sorting, including safe, chunks, and cycles.
 */
export function graphSequencer<T> (graph: Graph<T>, includedNodes: T[] = [...graph.keys()]): Result<T> {
  const cycles: T[][] = []
  const chunks: T[][] = []
  const visited = new Set<T>()
  const nodes = new Set<T>(includedNodes)
  const outDegree = new Map<T, number>()
  const reverseGraph = new Map<T, T[]>()
  let safe = true
  // Function to update the outDegree of a node.
  const changeDegree = (node: T, v: number) => {
    const degree = outDegree.get(node) ?? 0
    outDegree.set(node, degree + v)
  }

  // Initialize reverseGraph with empty arrays for all nodes.
  for (const key of graph.keys()) {
    reverseGraph.set(key, [])
  }

  // Calculate outDegree and reverseGraph for the included nodes.
  for (const [from, edges] of graph.entries()) {
    outDegree.set(from, 0)
    for (const to of edges) {
      if (nodes.has(from) && nodes.has(to)) {
        changeDegree(from, 1)
        const reverseEdges = reverseGraph.get(to)!
        reverseGraph.set(to, [...reverseEdges, from])
      }
    }

    if (!nodes.has(from)) {
      visited.add(from)
    }
  }

  // Function to remove a node from the graph.
  const removeNode = (node: T) => {
    for (const from of reverseGraph.get(node)!) {
      changeDegree(from, -1)
    }
    visited.add(node)
    nodes.delete(node)
  }

  const findCycle = (startNode: T): T[] => {
    const queue: Array<[T, T[]]> = [[startNode, [startNode]]]
    const cycleVisited = new Set<T>()
    while (queue.length) {
      const [id, cycle] = queue.shift()!
      for (const to of graph.get(id)!) {
        if (to === startNode) {
          return cycle
        }
        if (visited.has(to) || cycleVisited.has(to)) {
          continue
        }
        cycleVisited.add(to)
        queue.push([to, [...cycle, to]])
      }
    }
    return []
  }

  while (nodes.size) {
    const chunk: T[] = []
    let minDegree = Number.MAX_SAFE_INTEGER
    for (const node of nodes) {
      const degree = outDegree.get(node)!
      if (degree === 0) {
        chunk.push(node)
      }
      minDegree = Math.min(minDegree, degree)
    }

    if (minDegree === 0) {
      chunk.forEach(removeNode)
      chunks.push(chunk)
    } else {
      let cycleNodes: T[] = []
      for (const node of nodes) {
        const cycle = findCycle(node)
        if (cycle.length) {
          cycles.push(cycle)
          cycle.forEach(removeNode)
          cycleNodes = cycleNodes.concat(cycle)

          if (cycle.length > 1) {
            safe = false
          }
        }
      }
      chunks.push(cycleNodes)
    }
  }

  return { safe, chunks, cycles }
}
