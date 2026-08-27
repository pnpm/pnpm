export type Graph<T> = Map<T, T[]>
export type Groups<T> = T[][]

export interface Options<T> {
  graph: Graph<T>
  groups: Groups<T>
}

export interface Result<T> {
  order: T[]
  cycles: Groups<T>
}

/**
 * Performs topological sorting on a graph while supporting node restrictions.
 *
 * The nodes are interned to indices up front and each ready set is gathered
 * from nodes whose degree a removal drops to zero, so a workspace-scale graph
 * sorts in O(V log V + E) instead of repeatedly scanning every node. Cycle discovery is confined
 * to each strongly connected component: nodes that merely lead into a cycle
 * cost nothing extra, and only enumerating the cycles inside one component
 * pays that component's size per reported cycle (the price of the
 * established cycle-reporting semantics).
 *
 * @param {Graph<T>}  graph - The graph represented as a Map where keys are nodes and values are their outgoing edges.
 * @param {T[]} includedNodes - An array of nodes that should be included in the sorting process. Other nodes will be ignored.
 * @returns {Result<T>} An object containing one deterministic order and the cycles encountered.
 */
export function graphSequencer<T> (graph: Graph<T>, includedNodes: T[] = [...graph.keys()]): Result<T> {
  // Included nodes are interned first, so an id below includedCount is an
  // included node and id order follows includedNodes.
  const indexOf = new Map<T, number>()
  const nodes: T[] = []
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

  // A non-included node is born removed: the order never contains it and the
  // cycle search does not walk through it.
  const removed: boolean[] = nodes.map((_, id) => id >= includedCount)

  const order: number[] = []
  const cycles: number[][] = []

  let remaining = includedCount
  // The ids whose degree is zero, i.e. the next ready set. Kept sorted in
  // includedNodes order.
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
      //
      // A cycle through a node lies entirely inside the node's strongly
      // connected component, so only members of a non-trivial component
      // (or self-loops) are searched, and each search stays inside its
      // component. Without the filter, every node that merely leads
      // *into* a cycle pays a full reachability walk that finds nothing.
      const components = computeStronglyConnectedComponents(adjacency, removed)
      const cycleIds: number[] = []
      for (let id = 0; id < includedCount; id++) {
        if (removed[id] || !mayLieOnCycle(components, id)) {
          continue
        }
        const cycle = findCycle(id, components)
        if (cycle.length === 0) {
          continue
        }
        for (const node of cycle) {
          removeNode(node)
        }
        // Appended one by one: a call-spread turns every cycle member into
        // a function argument, and a pathological workspace-sized cycle
        // would overflow the engine's argument limit.
        for (const node of cycle) {
          cycleIds.push(node)
        }
        cycles.push(cycle)
      }
      remaining -= cycleIds.length
      for (const id of cycleIds) order.push(id)
    } else {
      for (const id of current) {
        removeNode(id)
      }
      remaining -= current.length
      for (const id of current) order.push(id)
    }
    // Breaking a cycle removes its members one by one, so an earlier
    // member's removal can drop a later member to degree zero right before
    // that member is removed too — filter those out of the zero-degree set
    // instead of adding them to the order twice.
    current = next.filter((id) => !removed[id]).sort((left, right) => left - right)
  }

  return {
    order: order.map((id) => nodes[id]),
    cycles: cycles.map((cycle) => cycle.map((id) => nodes[id])),
  }

  function intern (node: T): number {
    let id = indexOf.get(node)
    if (id === undefined) {
      id = nodes.length
      indexOf.set(node, id)
      nodes.push(node)
    }
    return id
  }

  // The longest of the shortest cycles running from startId back to itself
  // through nodes not yet removed, or empty when there is none. The walk
  // stays inside startId's strongly connected component — no cycle through
  // startId can leave it.
  function findCycle (startId: number, components: StronglyConnectedComponents): number[] {
    const queue: Array<[number, number[]]> = [[startId, [startId]]]
    let head = 0
    const cycleVisited = new Set<number>()
    const foundCycles: number[][] = []

    while (head < queue.length) {
      const [id, cycle] = queue[head++]
      for (const to of adjacency[id]) {
        if (to === startId) {
          cycleVisited.add(to)
          foundCycles.push([...cycle])
          continue
        }
        if (removed[to] || cycleVisited.has(to) || components.componentOf[to] !== components.componentOf[startId]) {
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

  // Whether a cycle through the node can exist: it shares a non-trivial
  // component with another node, or loops onto itself. Removals since the
  // components were computed can make this a false positive — the search
  // then comes back empty, exactly as it would have without the filter —
  // but never a false negative, because removals only take cycles away.
  function mayLieOnCycle (components: StronglyConnectedComponents, id: number): boolean {
    return components.componentSize[components.componentOf[id]] >= 2 || adjacency[id].includes(id)
  }
}

// The strongly connected components of the not-yet-removed subgraph,
// computed with an iterative Tarjan walk (recursion would overflow on a
// workspace-deep chain). Removed nodes belong to no component.
interface StronglyConnectedComponents {
  componentOf: number[]
  componentSize: number[]
}

function computeStronglyConnectedComponents (adjacency: number[][], removed: boolean[]): StronglyConnectedComponents {
  const nodeCount = adjacency.length
  const NONE = -1
  const discovery: number[] = new Array(nodeCount).fill(NONE)
  const lowLink: number[] = new Array(nodeCount).fill(0)
  const onStack: boolean[] = new Array(nodeCount).fill(false)
  const stack: number[] = []
  const componentOf: number[] = new Array(nodeCount).fill(NONE)
  const componentSize: number[] = []
  let nextDiscovery = 0
  // Explicit DFS frames of [node, next edge position].
  const frames: Array<[number, number]> = []

  for (let root = 0; root < nodeCount; root++) {
    if (removed[root] || discovery[root] !== NONE) {
      continue
    }
    discovery[root] = nextDiscovery
    lowLink[root] = nextDiscovery
    nextDiscovery++
    stack.push(root)
    onStack[root] = true
    frames.push([root, 0])
    while (frames.length > 0) {
      const frame = frames[frames.length - 1]
      const node = frame[0]
      const edgeIndex = frame[1]
      frame[1]++
      if (edgeIndex < adjacency[node].length) {
        const to = adjacency[node][edgeIndex]
        if (removed[to]) {
          continue
        }
        if (discovery[to] === NONE) {
          discovery[to] = nextDiscovery
          lowLink[to] = nextDiscovery
          nextDiscovery++
          stack.push(to)
          onStack[to] = true
          frames.push([to, 0])
        } else if (onStack[to]) {
          lowLink[node] = Math.min(lowLink[node], discovery[to])
        }
      } else {
        frames.pop()
        if (frames.length > 0) {
          const parent = frames[frames.length - 1][0]
          lowLink[parent] = Math.min(lowLink[parent], lowLink[node])
        }
        if (lowLink[node] === discovery[node]) {
          const component = componentSize.length
          let size = 0
          for (;;) {
            const member = stack.pop()!
            onStack[member] = false
            componentOf[member] = component
            size++
            if (member === node) {
              break
            }
          }
          componentSize.push(size)
        }
      }
    }
  }

  return { componentOf, componentSize }
}
