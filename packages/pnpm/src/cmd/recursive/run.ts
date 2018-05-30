import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import graphSequencer = require('graph-sequencer')
import createPkgGraph from 'pkgs-graph'
import {sync as runScriptSync} from '../../runScript'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
) => {
  const pkgGraphResult = createPkgGraph(pkgs)
  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  const chunks = graphSequencerResult.chunks

  // TODO: run chunks concurrently
  for (const chunk of chunks) {
    for (const prefix of chunk) {
      const pkg = pkgGraphResult.graph[prefix] as {manifest: PackageJson, path: string}
      if (!pkg.manifest.scripts || !pkg.manifest.scripts[args[0]]) {
        continue
      }
      try {
        const result = runScriptSync('npm', ['run'].concat(args), {
          cwd: prefix,
          stdio: 'inherit',
          userAgent: undefined,
        })
        if (result.status !== 0) {
          throw new Error(`Running "${args.join(' ')}" failed with exit code "${result.status}"`)
        }
      } catch (err) {
        logger.info(err)
        err['prefix'] = prefix // tslint:disable-line:no-string-literal
        throw err
      }
    }
  }
}
