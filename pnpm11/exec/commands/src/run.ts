import type { CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli.utils'
import { type Config, type ConfigContext, getWorkspaceConcurrency, types as allTypes } from '@pnpm/config.reader'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { keepEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import type { PackageScripts, ProjectManifest, ProjectsGraph } from '@pnpm/types'
import pLimit from 'p-limit'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { buildCommandNotFoundHint } from './buildCommandNotFoundHint.js'
import { createLifecycleOpts, suppressesScriptEcho } from './createLifecycleOpts.js'
import { handler as exec } from './exec.js'
import { throwOrFilterHiddenScripts } from './hiddenScripts.js'
import { printProjectCommands } from './printProjectCommands.js'
import { runDepsStatusCheck } from './runDepsStatusCheck.js'
import { getSpecifiedScripts as getSpecifiedScriptWithoutStartCommand, type RecursiveRunOpts, runRecursive } from './runRecursive.js'
import { getRunScriptCommands, runScript, type RunScriptOptions } from './runScript.js'

export { getRunScriptCommands, runScript, type RunScriptOptions, suppressesScriptEcho }

export const IF_PRESENT_OPTION: Record<string, unknown> = {
  'if-present': Boolean,
}

export interface DescriptionItem {
  shortAlias?: string
  name: string
  description?: string
}

export const IF_PRESENT_OPTION_HELP: DescriptionItem = {
  description: 'Avoid exiting with a non-zero exit code when the script is undefined',
  name: '--if-present',
}

export const PARALLEL_OPTION_HELP: DescriptionItem = {
  description: 'Completely disregard concurrency and topological sorting, \
running a given script immediately in all matching packages \
with prefixed streaming output. This is the preferred flag \
for long-running processes such as watch run over many packages.',
  name: '--parallel',
}

export const RESUME_FROM_OPTION_HELP: DescriptionItem = {
  description: 'Command executed from given package',
  name: '--resume-from',
}

export const SEQUENTIAL_OPTION_HELP: DescriptionItem = {
  description: 'Run the specified scripts one by one',
  name: '--sequential',
  shortAlias: '-s',
}

export const REPORT_SUMMARY_OPTION_HELP: DescriptionItem = {
  description: 'Save the execution results of every package to "pnpm-exec-summary.json". Useful to inspect the execution time and status of each package.',
  name: '--report-summary',
}

export const REPORTER_HIDE_PREFIX_HELP: DescriptionItem = {
  description: 'Hide project name prefix from output of running scripts. Useful when running in CI like GitHub Actions and the output from a script may create an annotation.',
  name: '--reporter-hide-prefix',
}

export const DRY_RUN_OPTION_HELP: DescriptionItem = {
  description: 'Print the task graph a recursive run would execute, without running anything. With "--json", prints the tasks and their resolved dependency edges as JSON',
  name: '--dry-run',
}

export const shorthands: Record<string, string[]> = {
  parallel: [
    '--workspace-concurrency=Infinity',
    '--no-sort',
    '--stream',
    '--recursive',
  ],
  s: [
    '--sequential',
    '--workspace-concurrency=1',
  ],
  sequential: [
    '--workspace-concurrency=1',
  ],
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'npm-path',
      'node-experimental-package-map',
      'node-package-map-type',
    ], allTypes),
  }
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'bail',
      'sort',
      'unsafe-perm',
      'workspace-concurrency',
      'scripts-prepend-node-path',
    ], allTypes),
    ...IF_PRESENT_OPTION,
    'dry-run': Boolean,
    json: Boolean,
    recursive: Boolean,
    reverse: Boolean,
    'resume-from': String,
    'report-summary': Boolean,
    'reporter-hide-prefix': Boolean,
    sequential: Boolean,
  }
}

export const completion: CompletionFunc = async (cliOpts, params) => {
  if (params.length > 0) {
    return []
  }
  const manifest = await readProjectManifestOnly(cliOpts.dir as string ?? process.cwd(), cliOpts)
  return Object.keys(manifest.scripts ?? {}).map((name) => ({ name }))
}

export const commandNames = ['run', 'run-script']

export function help (): string {
  return renderHelp({
    aliases: ['run-script'],
    description: 'Runs a defined package script.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Run the defined package script in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Continue running the remaining scripts even if one of them fails, instead of aborting on the first failure. The command still exits with a non-zero exit code if any script failed',
            name: '--no-bail',
          },
          DRY_RUN_OPTION_HELP,
          IF_PRESENT_OPTION_HELP,
          PARALLEL_OPTION_HELP,
          RESUME_FROM_OPTION_HELP,
          ...UNIVERSAL_OPTIONS,
          SEQUENTIAL_OPTION_HELP,
          REPORT_SUMMARY_OPTION_HELP,
          REPORTER_HIDE_PREFIX_HELP,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('run'),
    usages: ['pnpm run <command> [<args>...]'],
  })
}

export type RunOpts =
  & Omit<RecursiveRunOpts, 'allProjects' | 'selectedProjectsGraph' | 'workspaceDir'>
  & { recursive?: boolean }
  & Pick<Config,
  | 'bin'
  | 'verifyDepsBeforeRun'
  | 'dir'
  | 'enablePrePostScripts'
  | 'engineStrict'
  | 'extendNodePath'
  | 'extraBinPaths'
  | 'extraEnv'
  | 'nodeOptions'
  | 'nodeExperimentalPackageMap'
  | 'pnpmHomeDir'
  | 'preferSymlinkedExecutables'
  | 'loglevel'
  | 'reporter'
  | 'scriptShell'
  | 'scriptsPrependNodePath'
  | 'shellEmulator'
  | 'syncInjectedDepsAfterScripts'
  | 'userAgent'
  >
  & Partial<Pick<Config, 'filter' | 'filterProd'>>
  & Pick<ConfigContext, 'cliOptions'>
  & Partial<Pick<ConfigContext, 'rawCliConfig'>>
  & (
    | { recursive?: false } & Partial<Pick<ConfigContext, 'allProjects' | 'selectedProjectsGraph'> & Pick<Config, 'workspaceDir'>>
    | { recursive: true } & Required<Pick<ConfigContext, 'allProjects' | 'selectedProjectsGraph'> & Pick<Config, 'workspaceDir'>>
  )
  & {
    argv?: {
      original: string[]
    }
    fallbackCommandUsed?: boolean
    sequential?: boolean
  }
  & CheckDepsStatusOptions

export async function handler (
  opts: RunOpts,
  params: string[]
): Promise<string | { exitCode: number } | undefined> {
  if (opts.sequential) {
    opts.workspaceConcurrency = 1
  }
  expandTestShorthand(opts, params)
  const [scriptName, ...passedThruArgs] = params
  await prepareRun(opts)

  if (opts.recursive && hasRecursiveWork(opts.selectedProjectsGraph, scriptName)) {
    return fallsBackToExec(opts, scriptName)
      // exec must not repeat the dependency verification above.
      ? exec({ implicitlyFellbackFromRun: true, ...opts, verifyDepsBeforeRun: false }, params)
      : runRecursive(params, opts)
  }
  const dir = opts.recursive ? Object.keys(opts.selectedProjectsGraph)[0] : opts.dir
  const manifest = await readProjectManifestOnly(dir, opts)
  if (!scriptName) {
    return printProjectCommands(manifest, await readOtherWorkspaceRootManifest(opts, dir))
  }
  const specifiedScripts = selectSpecifiedScripts(manifest, scriptName)
  if (specifiedScripts.length < 1) {
    return handleMissingScript(opts, scriptName, manifest)
  }
  await runProjectScripts(opts, { dir, manifest, specifiedScripts, passedThruArgs })
  return undefined
}

function expandTestShorthand (opts: RunOpts, params: string[]): void {
  if (opts.fallbackCommandUsed && (params[0] === 't' || params[0] === 'tst')) {
    params[0] = 'test'
  }
}

async function prepareRun (opts: RunOpts): Promise<void> {
  // Before the dependency verification: an unsupported flag must fail
  // before anything can trigger an install or a prompt.
  if (opts.dryRun && !opts.recursive) {
    throw new PnpmError('DRY_RUN_NOT_RECURSIVE', 'The --dry-run option is only supported with recursive runs', {
      hint: 'Use "pnpm -r run --dry-run <script>" to print the task graph of a recursive run.',
    })
  }

  // A dry run prints what would execute and runs nothing, so it must not
  // let the dependency verification trigger an install either.
  if (opts.verifyDepsBeforeRun && !opts.dryRun) {
    await runDepsStatusCheck(opts)
  }

  if (opts.nodeOptions) {
    opts.extraEnv = {
      ...opts.extraEnv,
      NODE_OPTIONS: keepEsmNodePathLoaderOption(opts.nodeOptions, opts.extraEnv?.NODE_OPTIONS),
    }
  }
}

function hasRecursiveWork (selectedProjectsGraph: ProjectsGraph, scriptName: string | undefined): boolean {
  return Boolean(scriptName) || Object.keys(selectedProjectsGraph).length > 1
}

async function readOtherWorkspaceRootManifest (opts: RunOpts, dir: string): Promise<ProjectManifest | undefined> {
  if (!opts.workspaceDir || opts.workspaceDir === dir) return undefined
  return (await tryReadProjectManifest(opts.workspaceDir, opts)).manifest ?? undefined
}

function selectSpecifiedScripts (manifest: ProjectManifest, scriptName: string): string[] {
  const specifiedScripts = getSpecifiedScripts(manifest.scripts ?? {}, scriptName)
  if (process.env.npm_lifecycle_event) return specifiedScripts
  return throwOrFilterHiddenScripts(specifiedScripts, scriptName)
}

async function handleMissingScript (
  opts: RunOpts,
  scriptName: string,
  manifest: ProjectManifest
): Promise<{ exitCode: number } | undefined> {
  if (opts.ifPresent) return undefined
  if (opts.fallbackCommandUsed) {
    return exec({
      selectedProjectsGraph: {},
      implicitlyFellbackFromRun: true,
      ...opts,
    }, getFallbackExecParams(opts))
  }
  await throwIfScriptIsInWorkspaceRoot(opts, scriptName)
  throw new PnpmError('NO_SCRIPT', `Missing script: ${scriptName}`, {
    hint: buildCommandNotFoundHint(scriptName, manifest.scripts),
  })
}

function getFallbackExecParams (opts: RunOpts): string[] {
  if (opts.argv == null) throw new Error('Could not fallback because opts.argv.original was not passed to the script runner')
  const params = opts.argv.original.slice(1)
  while (params.length > 0 && params[0][0] === '-' && params[0] !== '--') {
    params.shift()
  }
  if (params.length > 0 && params[0] === '--') {
    params.shift()
  }
  if (params.length === 0) {
    throw new PnpmError('UNEXPECTED_BEHAVIOR', 'Params should not be an empty array', {
      hint: 'This was a bug caused by programmer error. Please report it',
    })
  }
  return params
}

async function throwIfScriptIsInWorkspaceRoot (opts: RunOpts, scriptName: string): Promise<void> {
  if (!opts.workspaceDir) return
  const { manifest: rootManifest } = await tryReadProjectManifest(opts.workspaceDir, opts)
  if (getSpecifiedScripts(rootManifest?.scripts ?? {}, scriptName).length > 0) {
    throw new PnpmError('NO_SCRIPT', `Missing script: ${scriptName}`, {
      hint: `But script matched with ${scriptName} is present in the root of the workspace,
so you may run "pnpm -w run ${scriptName}"`,
    })
  }
}

interface ProjectScripts {
  dir: string
  manifest: ProjectManifest
  specifiedScripts: string[]
  passedThruArgs: string[]
}

async function runProjectScripts (opts: RunOpts, project: ProjectScripts): Promise<void> {
  const { specifiedScripts } = project
  const concurrency = getWorkspaceConcurrency(opts.workspaceConcurrency)
  const lifecycleOpts = await createLifecycleOpts(opts, {
    dir: project.dir,
    manifest: project.manifest,
    stdio: (specifiedScripts.length > 1 && concurrency > 1) ? 'pipe' : 'inherit',
  })
  const limitRun = pLimit(concurrency)

  const runScriptOptions: RunScriptOptions = {
    enablePrePostScripts: opts.enablePrePostScripts ?? false,
    syncInjectedDepsAfterScripts: opts.syncInjectedDepsAfterScripts,
    workspaceDir: opts.workspaceDir,
  }
  const _runScript = runScript.bind(null, { manifest: project.manifest, lifecycleOpts, runScriptOptions, passedThruArgs: project.passedThruArgs })

  if (opts.bail !== false) {
    await Promise.all(specifiedScripts.map(script => limitRun(() => _runScript(script))))
    return
  }
  const results = await Promise.allSettled(
    specifiedScripts.map(script => limitRun(() => _runScript(script)))
  )
  throwIfSomeScriptsFailed(results, specifiedScripts)
}

function throwIfSomeScriptsFailed (results: Array<PromiseSettledResult<void>>, specifiedScripts: string[]): void {
  const failures = results
    .map((result, index) => ({ result, script: specifiedScripts[index] }))
    .filter((entry): entry is { result: PromiseRejectedResult, script: string } => entry.result.status === 'rejected')
  if (failures.length > 0) {
    throw new PnpmError(
      'RUN_FAILED',
      `Some scripts failed: ${failures.length} of ${specifiedScripts.length}`,
      {
        hint: failures
          .map(({ script, result }) => `${script}: ${result.reason?.message ?? String(result.reason)}`)
          .join('\n'),
      }
    )
  }
}

/**
 * Whether a recursive `pnpm <command>` shorthand hands the command to `exec`,
 * as the single-project shorthand does when no selected project has a script
 * by that name. `test` and `start` have defaults of their own when the script
 * is missing, so they are never handed to `exec` as binaries.
 */
function fallsBackToExec (opts: RunOpts & { recursive: true }, scriptName: string): boolean {
  return Boolean(opts.fallbackCommandUsed) &&
    scriptName !== 'test' &&
    scriptName !== 'start' &&
    !opts.ifPresent &&
    !opts.dryRun &&
    !someSelectedProjectHasScript(opts.selectedProjectsGraph, scriptName)
}

function someSelectedProjectHasScript (selectedProjectsGraph: ProjectsGraph, scriptName: string): boolean {
  return Object.values(selectedProjectsGraph).some(({ package: { manifest } }) =>
    getSpecifiedScriptWithoutStartCommand(manifest.scripts ?? {}, scriptName).length > 0
  )
}

function getSpecifiedScripts (scripts: PackageScripts, scriptName: string): string[] {
  const specifiedSelector = getSpecifiedScriptWithoutStartCommand(scripts, scriptName)

  if (specifiedSelector.length > 0) {
    return specifiedSelector
  }

  // if a user passes start command as scriptName, `node server.js` will be executed as a fallback, so return start command even if start command is not defined in package.json
  if (scriptName === 'start') {
    return [scriptName]
  }

  return []
}
