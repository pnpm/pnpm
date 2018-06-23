import {PackageJson} from '@pnpm/types'
import graphSequencer = require('graph-sequencer')
import createPkgGraph from 'pkgs-graph'

export default (pkgs: Array<{path: string, manifest: PackageJson}>) => {
  const pkgGraphResult = createPkgGraph(pkgs)
  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  return {
    chunks: graphSequencerResult.chunks,
    graph: pkgGraphResult.graph,
  }
}
