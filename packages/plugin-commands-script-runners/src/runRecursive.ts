import { RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, ProjectsGraph } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import runLifecycleHooks from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import sortPackages from '@pnpm/sort-packages'
import { PackageManifest } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import pLimit from 'p-limit'

export type RecursiveRunOpts = Pick<Config,
  'unsafePerm' |
  'rawConfig'
> & Required<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>> &
Partial<Pick<Config, 'extraBinPaths' | 'bail' | 'sort' | 'workspaceConcurrency'>>

export default async (
  args: string[],
  opts: RecursiveRunOpts,
) => {
  const scriptName = args[0]
  let hasCommand = 0
  const packageChunks = opts.sort
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)
  const stdio = (
    opts.workspaceConcurrency === 1 ||
    packageChunks.length === 1 && packageChunks[0].length === 1
  ) ? 'inherit' : 'pipe'
  const passedThruArgs = args.slice(1)

  for (const chunk of packageChunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        const pkg = opts.selectedProjectsGraph[prefix] as {package: {dir: string, manifest: PackageManifest}}
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
    const allPackagesAreSelected = Object.keys(opts.selectedProjectsGraph).length === opts.allProjects.length
    if (allPackagesAreSelected) {
      throw new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the packages has a "${scriptName}" script`)
    } else {
      logger.info({
        message: `None of the selected packages has a "${scriptName}" script`,
        prefix: opts.workspaceDir
      })
    }
  }

  throwOnCommandFail('pnpm recursive run', result)
}
