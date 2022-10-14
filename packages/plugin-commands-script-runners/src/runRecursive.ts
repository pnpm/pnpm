import path from 'path'
import { RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import {
  runLifecycleHook,
  makeNodeRequireOption,
  RunLifecycleHookOptions,
} from '@pnpm/lifecycle'
import { logger } from '@pnpm/logger'
import { sortPackages } from '@pnpm/sort-packages'
import pLimit from 'p-limit'
import realpathMissing from 'realpath-missing'
import existsInDir from './existsInDir'

export type RecursiveRunOpts = Pick<Config,
| 'enablePrePostScripts'
| 'unsafePerm'
| 'rawConfig'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'shellEmulator'
| 'stream'
> & Required<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>> &
Partial<Pick<Config, 'extraBinPaths' | 'extraEnv' | 'bail' | 'reverse' | 'sort' | 'workspaceConcurrency'>> &
{
  ifPresent?: boolean
}

export default async (
  params: string[],
  opts: RecursiveRunOpts
) => {
  const [scriptName, ...passedThruArgs] = params
  if (!scriptName) {
    throw new PnpmError('SCRIPT_NAME_IS_REQUIRED', 'You must specify the script you want to run')
  }
  let hasCommand = 0

  const sortedPackageChunks = opts.sort
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]
  const packageChunks = opts.reverse ? sortedPackageChunks.reverse() : sortedPackageChunks

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)
  const stdio =
    !opts.stream &&
    (opts.workspaceConcurrency === 1 ||
      (packageChunks.length === 1 && packageChunks[0].length === 1))
      ? 'inherit'
      : 'pipe'
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  for (const chunk of packageChunks) {
    await Promise.all(chunk.map(async (prefix: string) =>
      limitRun(async () => {
        const pkg = opts.selectedProjectsGraph[prefix]
        if (
          !pkg.package.manifest.scripts?.[scriptName] ||
          process.env.npm_lifecycle_event === scriptName &&
          process.env.PNPM_SCRIPT_SRC_DIR === prefix
        ) {
          return
        }
        hasCommand++
        try {
          const lifecycleOpts: RunLifecycleHookOptions = {
            depPath: prefix,
            extraBinPaths: opts.extraBinPaths,
            extraEnv: opts.extraEnv,
            pkgRoot: prefix,
            rawConfig: opts.rawConfig,
            rootModulesDir: await realpathMissing(path.join(prefix, 'node_modules')),
            scriptsPrependNodePath: opts.scriptsPrependNodePath,
            scriptShell: opts.scriptShell,
            shellEmulator: opts.shellEmulator,
            stdio,
            unsafePerm: true, // when running scripts explicitly, assume that they're trusted.
          }
          const pnpPath = workspacePnpPath ?? await existsPnp(prefix)
          if (pnpPath) {
            lifecycleOpts.extraEnv = {
              ...lifecycleOpts.extraEnv,
              ...makeNodeRequireOption(pnpPath),
            }
          }
          if (
            opts.enablePrePostScripts &&
            pkg.package.manifest.scripts?.[`pre${scriptName}`] &&
            !pkg.package.manifest.scripts[scriptName].includes(`pre${scriptName}`)
          ) {
            await runLifecycleHook(`pre${scriptName}`, pkg.package.manifest, lifecycleOpts)
          }
          await runLifecycleHook(scriptName, pkg.package.manifest, { ...lifecycleOpts, args: passedThruArgs })
          if (
            opts.enablePrePostScripts &&
            pkg.package.manifest.scripts?.[`post${scriptName}`] &&
            !pkg.package.manifest.scripts[scriptName].includes(`post${scriptName}`)
          ) {
            await runLifecycleHook(`post${scriptName}`, pkg.package.manifest, lifecycleOpts)
          }
          result.passes++
        } catch (err: any) { // eslint-disable-line
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix,
            })
            return
          }

          err['code'] = 'ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL'
          err['prefix'] = prefix
          /* eslint-enable @typescript-eslint/dot-notation */
          throw err
        }
      }
      )))
  }

  if (scriptName !== 'test' && !hasCommand && !opts.ifPresent) {
    const allPackagesAreSelected = Object.keys(opts.selectedProjectsGraph).length === opts.allProjects.length
    if (allPackagesAreSelected) {
      throw new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `None of the packages has a "${scriptName}" script`)
    } else {
      logger.info({
        message: `None of the selected packages has a "${scriptName}" script`,
        prefix: opts.workspaceDir,
      })
    }
  }

  throwOnCommandFail('pnpm recursive run', result)
}
