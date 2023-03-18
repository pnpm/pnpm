import path from 'path'
import { throwOnCommandFail } from '@pnpm/cli-utils'
import { type Config } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import {
  makeNodeRequireOption,
  type RunLifecycleHookOptions,
} from '@pnpm/lifecycle'
import { logger } from '@pnpm/logger'
import { sortPackages } from '@pnpm/sort-packages'
import pLimit from 'p-limit'
import realpathMissing from 'realpath-missing'
import { existsInDir } from './existsInDir'
import { createEmptyRecursiveSummary, getExecutionDuration, getResumedPackageChunks, writeRecursiveSummary } from './exec'
import { runScript } from './run'
import { tryBuildRegExpFromCommand } from './regexpCommand'
import { type PackageScripts } from '@pnpm/types'

export type RecursiveRunOpts = Pick<Config,
| 'enablePrePostScripts'
| 'unsafePerm'
| 'rawConfig'
| 'rootProjectManifest'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'shellEmulator'
| 'stream'
> & Required<Pick<Config, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir' | 'dir'>> &
Partial<Pick<Config, 'extraBinPaths' | 'extraEnv' | 'bail' | 'reverse' | 'sort' | 'workspaceConcurrency'>> &
{
  ifPresent?: boolean
  resumeFrom?: string
  reportSummary?: boolean
}

export async function runRecursive (
  params: string[],
  opts: RecursiveRunOpts
) {
  const [scriptName, ...passedThruArgs] = params
  if (!scriptName) {
    throw new PnpmError('SCRIPT_NAME_IS_REQUIRED', 'You must specify the script you want to run')
  }
  let hasCommand = 0

  const sortedPackageChunks = opts.sort
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]
  let packageChunks = opts.reverse ? sortedPackageChunks.reverse() : sortedPackageChunks

  if (opts.resumeFrom) {
    packageChunks = getResumedPackageChunks({
      resumeFrom: opts.resumeFrom,
      chunks: packageChunks,
      selectedProjectsGraph: opts.selectedProjectsGraph,
    })
  }

  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)
  const stdio =
    !opts.stream &&
    (opts.workspaceConcurrency === 1 ||
      (packageChunks.length === 1 && packageChunks[0].length === 1))
      ? 'inherit'
      : 'pipe'
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  const requiredScripts = opts.rootProjectManifest?.pnpm?.requiredScripts ?? []
  if (requiredScripts.includes(scriptName)) {
    const missingScriptPackages: string[] = packageChunks
      .flat()
      .map((prefix) => opts.selectedProjectsGraph[prefix])
      .filter((pkg) => getSpecifiedScripts(pkg.package.manifest.scripts ?? {}, scriptName).length < 1)
      .map((pkg) => pkg.package.manifest.name ?? pkg.package.dir)
    if (missingScriptPackages.length) {
      throw new PnpmError('RECURSIVE_RUN_NO_SCRIPT', `Missing script "${scriptName}" in packages: ${missingScriptPackages.join(', ')}`)
    }
  }

  const result = createEmptyRecursiveSummary(packageChunks)

  for (const chunk of packageChunks) {
    const selectedScripts = chunk.map(prefix => {
      const pkg = opts.selectedProjectsGraph[prefix]
      const specifiedScripts = getSpecifiedScripts(pkg.package.manifest.scripts ?? {}, scriptName)
      if (!specifiedScripts.length) {
        result[prefix].status = 'skipped'
      }
      return specifiedScripts.map(script => ({ prefix, scriptName: script }))
    }).flat()

    await Promise.all(selectedScripts.map(async ({ prefix, scriptName }) =>
      limitRun(async () => {
        const pkg = opts.selectedProjectsGraph[prefix]
        if (
          !pkg.package.manifest.scripts?.[scriptName] ||
          process.env.npm_lifecycle_event === scriptName &&
          process.env.PNPM_SCRIPT_SRC_DIR === prefix
        ) {
          return
        }
        result[prefix].status = 'running'
        const startTime = process.hrtime()
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

          const _runScript = runScript.bind(null, { manifest: pkg.package.manifest, lifecycleOpts, runScriptOptions: { enablePrePostScripts: opts.enablePrePostScripts ?? false }, passedThruArgs })
          await _runScript(scriptName)
          result[prefix].status = 'passed'
          result[prefix].duration = getExecutionDuration(startTime)
        } catch (err: any) { // eslint-disable-line
          logger.info(err)

          result[prefix] = {
            status: 'failure',
            duration: getExecutionDuration(startTime),
            error: err,
            message: err.message,
            prefix,
          }

          if (!opts.bail) {
            return
          }

          err['code'] = 'ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL'
          err['prefix'] = prefix
          opts.reportSummary && await writeRecursiveSummary({
            dir: opts.workspaceDir ?? opts.dir,
            summary: result,
          })
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
  opts.reportSummary && await writeRecursiveSummary({
    dir: opts.workspaceDir ?? opts.dir,
    summary: result,
  })
  throwOnCommandFail('pnpm recursive run', result)
}

export function getSpecifiedScripts (scripts: PackageScripts, scriptName: string) {
  // if scripts in package.json has script which is equal to scriptName a user passes, return it.
  if (scripts[scriptName]) {
    return [scriptName]
  }

  const scriptSelector = tryBuildRegExpFromCommand(scriptName)

  // if scriptName which a user passes is RegExp (like /build:.*/), multiple scripts to execute will be selected with RegExp
  if (scriptSelector) {
    const scriptKeys = Object.keys(scripts)
    return scriptKeys.filter(script => script.match(scriptSelector))
  }

  return []
}
