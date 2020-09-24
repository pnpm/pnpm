import { ProjectsGraph } from '@pnpm/types'
import graphSequencer = require('graph-sequencer')

export default function sortPackages (pkgGraph: ProjectsGraph): string[][] {
  const keys = Object.keys(pkgGraph)
  const setOfKeys = new Set(keys)
  const graph = new Map(
    keys.map((pkgPath) => [
      pkgPath,
      pkgGraph[pkgPath].dependencies.filter(
        /* remove cycles of length 1 (ie., package 'a' depends on 'a').  They
        confuse the graph-sequencer, but can be ignored when ordering packages
        topologically.

        See the following example where 'b' and 'c' depend on themselves:

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['b', ['b']],
            ['c', ['b', 'c']]]
          ),
          groups: [['a', 'b', 'c']]})

        returns chunks:

            [['b'],['a'],['c']]

        But both 'b' and 'c' should be executed _before_ 'a', because 'a' depends on
        them.  It works (and is considered 'safe' if we run:)

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['b', []],
            ['c', ['b']]]
          ), groups: [['a', 'b', 'c']]})

        returning:

            [['b'], ['c'], ['a']]

        */
        d => d !== pkgPath &&
        /* remove unused dependencies that we can ignore due to a filter expression.

        Again, the graph sequencer used to behave weirdly in the following edge case:

          graphSequencer({graph: new Map([
            ['a', ['b', 'c']],
            ['d', ['a']],
            ['e', ['a', 'b', 'c']]]
          ),
          groups: [['a', 'e', 'e']]})

        returns chunks:

            [['d'],['a'],['e']]

        But we really want 'a' to be executed first.
        */
        setOfKeys.has(d))]
    )
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [keys],
  })
  return graphSequencerResult.chunks
}
