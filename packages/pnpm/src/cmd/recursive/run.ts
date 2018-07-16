import runLifecycleHooks from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import {realNodeModulesDir} from '@pnpm/utils'
import pLimit = require('p-limit')
import dividePackagesToChunks from './dividePackagesToChunks'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    bail: boolean,
    concurrency: number,
    unsafePerm: boolean,
    rawNpmConfig: object,
  },
) => {
  const scriptName = args[0]
  const {chunks, graph} = dividePackagesToChunks(pkgs)
  let hasCommand = 0
  let failed = false

  const limitRun = pLimit(opts.concurrency)

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        const pkg = graph[prefix] as {manifest: PackageJson, path: string}
        if (!pkg.manifest.scripts || !pkg.manifest.scripts[scriptName]) {
          return
        }
        hasCommand++
        try {
          const lifecycleOpts = {
            depPath: prefix,
            pkgRoot: prefix,
            rawNpmConfig: opts.rawNpmConfig,
            rootNodeModulesDir: await realNodeModulesDir(prefix),
            unsafePerm: opts.unsafePerm || false,
          }
          if (pkg.manifest.scripts[`pre${scriptName}`]) {
            await runLifecycleHooks(`pre${scriptName}`, pkg.manifest, lifecycleOpts)
          }
          await runLifecycleHooks(scriptName, pkg.manifest, lifecycleOpts)
          if (pkg.manifest.scripts[`post${scriptName}`]) {
            await runLifecycleHooks(`post${scriptName}`, pkg.manifest, lifecycleOpts)
          }
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            failed = true
            return
          }

          // tslint:disable:no-string-literal
          err['code'] = 'ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL'
          err['prefix'] = prefix
          // tslint:enable:no-string-literal
          throw err
        }
      },
    )))
  }

  if (failed) {
    const err = new Error('Recursive run failed')
    err['code'] = 'ERR_PNPM_RECURSIVE_RUN_FAIL' // tslint:disable-line:no-string-literal
    throw err
  }
  if (scriptName !== 'test' && !hasCommand) {
    const err = new Error(`None of the packages has a "${scriptName}" script`)
    err['code'] = 'RECURSIVE_RUN_NO_SCRIPT' // tslint:disable-line:no-string-literal
    throw err
  }
}
