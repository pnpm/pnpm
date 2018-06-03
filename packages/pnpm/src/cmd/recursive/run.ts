import runLifecycleHooks from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import {realNodeModulesDir} from '@pnpm/utils'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import createPkgGraph from 'pkgs-graph'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    concurrency: number,
    unsafePerm: boolean,
    rawNpmConfig: object,
  },
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
  let hasCommand = 0

  const limitRun = pLimit(opts.concurrency)

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        const pkg = pkgGraphResult.graph[prefix] as {manifest: PackageJson, path: string}
        if (!pkg.manifest.scripts || !pkg.manifest.scripts[args[0]]) {
          return
        }
        hasCommand++
        try {
          await runLifecycleHooks(
            args[0],
            pkg.manifest, {
              depPath: prefix,
              pkgRoot: prefix,
              rawNpmConfig: opts.rawNpmConfig,
              rootNodeModulesDir: await realNodeModulesDir(prefix),
              unsafePerm: opts.unsafePerm || false,
          })
        } catch (err) {
          logger.info(err)
          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          throw err
        }
      },
    )))
  }

  if (args[0] !== 'test' && !hasCommand) {
    const err = new Error(`None of the packages has a "${args[0]}" script`)
    err['code'] = 'RECURSIVE_RUN_NO_SCRIPT' // tslint:disable-line:no-string-literal
    throw err
  }
}
