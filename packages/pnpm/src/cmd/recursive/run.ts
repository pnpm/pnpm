import PnpmError from '@pnpm/error'
import runLifecycleHooks from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import { PackageManifest } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import pLimit from 'p-limit'
import { PackageNode } from 'pkgs-graph'
import RecursiveSummary from './recursiveSummary'

export default async <T> (
  packageChunks: string[][],
  graph: {[id: string]: PackageNode<T>},
  args: string[],
  cmd: string,
  opts: {
    bail: boolean,
    extraBinPaths: string[],
    workspaceConcurrency: number,
    unsafePerm: boolean,
    rawConfig: object,
    workspaceDir: string,
    allPackagesAreSelected: boolean,
  },
) => {
  const scriptName = args[0]
  let hasCommand = 0

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const limitRun = pLimit(opts.workspaceConcurrency)
  const stdio = (
    opts.workspaceConcurrency === 1 ||
    packageChunks.length === 1 && packageChunks[0].length === 1
  ) ? 'inherit' : 'pipe'
  const passedThruArgs = args.slice(1)

  for (const chunk of packageChunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        const pkg = graph[prefix] as {package: {manifest: PackageManifest, path: string}}
        if (!pkg.package.manifest.scripts || !pkg.package.manifest.scripts[scriptName]) {
          return
        }
        hasCommand++
        try {
          const lifecycleOpts = {
            depPath: prefix,
            extraBinPaths: opts.extraBinPaths,
            pkgRoot: prefix,
            rawConfig: opts.rawConfig,
            rootNodeModulesDir: await realNodeModulesDir(prefix),
            stdio,
            unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
          }
          if (pkg.package.manifest.scripts[`pre${scriptName}`]) {
            await runLifecycleHooks(`pre${scriptName}`, pkg.package.manifest, lifecycleOpts)
          }
          await runLifecycleHooks(scriptName, pkg.package.manifest, { ...lifecycleOpts, args: passedThruArgs })
          if (pkg.package.manifest.scripts[`post${scriptName}`]) {
            await runLifecycleHooks(`post${scriptName}`, pkg.package.manifest, lifecycleOpts)
          }
          result.passes++
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix,
            })
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

  if (scriptName !== 'test' && !hasCommand) {
    if (opts.allPackagesAreSelected) {
      throw new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the packages has a "${scriptName}" script`)
    } else {
      logger.info({
        message: `None of the selected packages has a "${scriptName}" script`,
        prefix: opts.workspaceDir
      })
    }
  }

  return result
}
