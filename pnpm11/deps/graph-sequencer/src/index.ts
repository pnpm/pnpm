export type Graph<Vertex> = Map<Vertex, Vertex[]>
export type Groups<Vertex> = Vertex[][]

export interface Options<Vertex> {
  graph: Graph<Vertex>
  groups: Groups<Vertex>
}

export interface Result<Vertex> {
  order: Vertex[]
  cycles: Groups<Vertex>
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
 * @param {Vertex[]} includedNodes - An array of nodes that should be included in the sorting process. Other nodes will be ignored.
 * @returns {Result<Vertex>} An object containing one deterministic order and the cycles encountered.
 */
export function graphSequencer<Vertex> (graph: Graph<Vertex>, includedNodes: Vertex[] = [...graph.keys()]): Result<Vertex> {
  const interned = internNodes(graph, includedNodes)
  const { order, cycles } = sequenceIds(createSequencerState(graph, interned))
  return {
    order: order.map((id) => interned.nodes[id]),
    cycles: cycles.map((cycle) => cycle.map((id) => interned.nodes[id])),
  }
}

interface InternedNodes<Vertex> {
  indexOf: Map<Vertex, number>
  nodes: Vertex[]
  // Included nodes are interned first, so an id below includedCount is an
  // included node and id order follows includedNodes.
  includedCount: number
}

function internNodes<Vertex> (graph: Graph<Vertex>, includedNodes: Vertex[]): InternedNodes<Vertex> {
  const interned: InternedNodes<Vertex> = { indexOf: new Map(), nodes: [], includedCount: 0 }
  for (const node of includedNodes) {
    intern(interned, node)
  }
  interned.includedCount = interned.nodes.length
  for (const [from, edges] of graph.entries()) {
    intern(interned, from)
    for (const to of edges) {
      intern(interned, to)
    }
  }
  return interned
}

function intern<Vertex> (interned: InternedNodes<Vertex>, node: Vertex): void {
  if (interned.indexOf.has(node)) return
  interned.indexOf.set(node, interned.nodes.length)
  interned.nodes.push(node)
}

interface SequencerState {
  adjacency: number[][]
  reverseGraph: number[][]
  outDegree: number[]
  // A non-included node is born removed: the order never contains it and the
  // cycle search does not walk through it.
  removed: boolean[]
  includedCount: number
}

function createSequencerState<Vertex> (graph: Graph<Vertex>, { indexOf, nodes, includedCount }: InternedNodes<Vertex>): SequencerState {
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
  return {
    adjacency,
    reverseGraph,
    outDegree,
    removed: nodes.map((_, id) => id >= includedCount),
    includedCount,
  }
}

function sequenceIds (state: SequencerState): { order: number[], cycles: number[][] } {
  const order: number[] = []
  const cycles: number[][] = []
  let remaining = state.includedCount
  // The ids whose degree is zero, i.e. the next ready set. Kept sorted in
  // includedNodes order.
  let current = collectInitialReadySet(state)
  while (remaining > 0) {
    const next: number[] = []
    const removeNode = (id: number) => {
      removeFromGraph(state, id, next)
    }
    const removedIds = current.length === 0
      ? breakCycles(state, removeNode, cycles)
      : removeReadySet(current, removeNode)
    remaining -= removedIds.length
    for (const id of removedIds) order.push(id)
    // Breaking a cycle removes its members one by one, so an earlier
    // member's removal can drop a later member to degree zero right before
    // that member is removed too — filter those out of the zero-degree set
    // instead of adding them to the order twice.
    current = next.filter((id) => !state.removed[id]).sort((left, right) => left - right)
  }
  return { order, cycles }
}

function collectInitialReadySet (state: SequencerState): number[] {
  const ready: number[] = []
  for (let id = 0; id < state.includedCount; id++) {
    if (state.outDegree[id] === 0) {
      ready.push(id)
    }
  }
  return ready
}

function removeFromGraph (state: SequencerState, id: number, next: number[]): void {
  state.removed[id] = true
  for (const parent of state.reverseGraph[id]) {
    if (state.outDegree[parent] <= 0) continue
    state.outDegree[parent]--
    if (state.outDegree[parent] === 0 && !state.removed[parent]) {
      next.push(parent)
    }
  }
}

function removeReadySet (ready: number[], removeNode: (id: number) => void): number[] {
  for (const id of ready) {
    removeNode(id)
  }
  return ready
}

// Every remaining node keeps a dependency alive: cycles. Break them the way
// the scan finds them, in includedNodes order.
//
// A cycle through a node lies entirely inside the node's strongly connected
// component, so only members of a non-trivial component (or self-loops) are
// searched, and each search stays inside its component. Without the filter,
// every node that merely leads *into* a cycle pays a full reachability walk
// that finds nothing.
function breakCycles (state: SequencerState, removeNode: (id: number) => void, cycles: number[][]): number[] {
  const components = computeStronglyConnectedComponents(state.adjacency, state.removed)
  const cycleIds: number[] = []
  for (let id = 0; id < state.includedCount; id++) {
    if (state.removed[id] || !mayLieOnCycle(state.adjacency, components, id)) {
      continue
    }
    const cycle = findCycle(state, id, components)
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
  return cycleIds
}

// The longest of the shortest cycles running from startId back to itself
// through nodes not yet removed, or empty when there is none. The walk
// stays inside startId's strongly connected component — no cycle through
// startId can leave it.
function findCycle (state: SequencerState, startId: number, components: StronglyConnectedComponents): number[] {
  const queue: Array<[number, number[]]> = [[startId, [startId]]]
  let head = 0
  const cycleVisited = new Set<number>()
  const foundCycles: number[][] = []

  while (head < queue.length) {
    const [id, cycle] = queue[head++]
    for (const to of state.adjacency[id]) {
      if (to === startId) {
        cycleVisited.add(to)
        foundCycles.push([...cycle])
        continue
      }
      if (state.removed[to] || cycleVisited.has(to) || components.componentOf[to] !== components.componentOf[startId]) {
        continue
      }
      cycleVisited.add(to)
      queue.push([to, [...cycle, to]])
    }
  }

  return pickLongestCycle(foundCycles)
}

function pickLongestCycle (foundCycles: number[][]): number[] {
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
function mayLieOnCycle (adjacency: number[][], components: StronglyConnectedComponents, id: number): boolean {
  return components.componentSize[components.componentOf[id]] >= 2 || adjacency[id].includes(id)
}

// The strongly connected components of the not-yet-removed subgraph,
// computed with an iterative Tarjan walk (recursion would overflow on a
// workspace-deep chain). Removed nodes belong to no component.
interface StronglyConnectedComponents {
  componentOf: number[]
  componentSize: number[]
}

const NONE = -1

interface TarjanWalk extends StronglyConnectedComponents {
  adjacency: number[][]
  removed: boolean[]
  discovery: number[]
  lowLink: number[]
  onStack: boolean[]
  stack: number[]
  nextDiscovery: number
  // Explicit DFS frames of [node, next edge position].
  frames: Array<[number, number]>
}

function computeStronglyConnectedComponents (adjacency: number[][], removed: boolean[]): StronglyConnectedComponents {
  const nodeCount = adjacency.length
  const walk: TarjanWalk = {
    adjacency,
    removed,
    discovery: new Array(nodeCount).fill(NONE),
    lowLink: new Array(nodeCount).fill(0),
    onStack: new Array(nodeCount).fill(false),
    stack: [],
    componentOf: new Array(nodeCount).fill(NONE),
    componentSize: [],
    nextDiscovery: 0,
    frames: [],
  }

  for (let root = 0; root < nodeCount; root++) {
    if (removed[root] || walk.discovery[root] !== NONE) {
      continue
    }
    discoverNode(walk, root)
    walkFrames(walk)
  }

  return { componentOf: walk.componentOf, componentSize: walk.componentSize }
}

function discoverNode (walk: TarjanWalk, node: number): void {
  walk.discovery[node] = walk.nextDiscovery
  walk.lowLink[node] = walk.nextDiscovery
  walk.nextDiscovery++
  walk.stack.push(node)
  walk.onStack[node] = true
  walk.frames.push([node, 0])
}

function walkFrames (walk: TarjanWalk): void {
  while (walk.frames.length > 0) {
    const frame = walk.frames[walk.frames.length - 1]
    const node = frame[0]
    const edgeIndex = frame[1]
    frame[1]++
    if (edgeIndex < walk.adjacency[node].length) {
      followEdge(walk, node, walk.adjacency[node][edgeIndex])
    } else {
      finishNode(walk, node)
    }
  }
}

function followEdge (walk: TarjanWalk, node: number, to: number): void {
  if (walk.removed[to]) {
    return
  }
  if (walk.discovery[to] === NONE) {
    discoverNode(walk, to)
  } else if (walk.onStack[to]) {
    walk.lowLink[node] = Math.min(walk.lowLink[node], walk.discovery[to])
  }
}

function finishNode (walk: TarjanWalk, node: number): void {
  walk.frames.pop()
  if (walk.frames.length > 0) {
    const parent = walk.frames[walk.frames.length - 1][0]
    walk.lowLink[parent] = Math.min(walk.lowLink[parent], walk.lowLink[node])
  }
  if (walk.lowLink[node] === walk.discovery[node]) {
    popComponent(walk, node)
  }
}

function popComponent (walk: TarjanWalk, root: number): void {
  const component = walk.componentSize.length
  let size = 0
  for (;;) {
    const member = walk.stack.pop()!
    walk.onStack[member] = false
    walk.componentOf[member] = component
    size++
    if (member === root) {
      break
    }
  }
  walk.componentSize.push(size)
}
