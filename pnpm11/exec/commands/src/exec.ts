import path from 'node:path'
import { StringDecoder } from 'node:string_decoder'

import { FILTERING, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readProjectManifestOnly, type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, getWorkspaceConcurrency, types } from '@pnpm/config.reader'
import { lifecycleLogger, type LifecycleMessage } from '@pnpm/core-loggers'
import type { CheckDepsStatusOptions } from '@pnpm/deps.status'
import { PnpmError } from '@pnpm/error'
import { keepEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import { makeNodePackageMapOption, makeNodeRequireOption } from '@pnpm/exec.lifecycle'
import { logger } from '@pnpm/logger'
import { prependDirsToPath } from '@pnpm/shell.path'
import type { Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { filteredProjectsDependencies } from '@pnpm/workspace.projects-sorter'
import {
  resumeTaskGraphFrom,
  reverseTaskGraph,
  scheduleTasks,
  sequenceTasks,
  type TaskCompletion,
  type TaskGraph,
  type TaskKey,
  taskKey,
  type TaskNode,
} from '@pnpm/workspace.task-scheduler'
import pLimit from 'p-limit'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import which from 'which'
import { writeJsonFile } from 'write-json-file'

import { getNearestProgram, getNearestScript } from './buildCommandNotFoundHint.js'
import { existsInDir } from './existsInDir.js'
import { makeEnv } from './makeEnv.js'
import {
  PARALLEL_OPTION_HELP,
  REPORT_SUMMARY_OPTION_HELP,
  RESUME_FROM_OPTION_HELP,
  shorthands as runShorthands,
} from './run.js'
import { runDepsStatusCheck } from './runDepsStatusCheck.js'
import { taskRunExecutionSettings, type TaskRunState, TaskRunStateContext } from './taskRunState.js'
import { trackedExeca } from './trackedExeca.js'

export const shorthands: Record<string, string | string[]> = {
  parallel: runShorthands.parallel,
  c: '--shell-mode',
}

export const commandNames = ['exec']

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'bail',
      'sort',
      'unsafe-perm',
      'workspace-concurrency',
      'reporter-hide-prefix',
      'node-experimental-package-map',
      'node-package-map-type',
    ], types),
    'shell-mode': Boolean,
    'resume-from': String,
    'report-summary': Boolean,
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  reverse: Boolean,
})

export function help (): string {
  return renderHelp({
    description: 'Run a shell command in the context of a project.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Do not hide project name prefix from output of recursively running command.',
            name: '--no-reporter-hide-prefix',
          },
          PARALLEL_OPTION_HELP,
          {
            description: 'Run the shell command in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'If exist, runs file inside of a shell. \
Uses /bin/sh on UNIX and \\cmd.exe on Windows. \
The shell should understand the -c switch on UNIX or /d /s /c on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
          RESUME_FROM_OPTION_HELP,
          REPORT_SUMMARY_OPTION_HELP,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('exec'),
    usages: ['pnpm [-r] [-c] exec <command> [args...]'],
  })
}

export async function writeRecursiveSummary (opts: { dir: string, summary: RecursiveSummary }): Promise<void> {
  await writeJsonFile(path.join(opts.dir, 'pnpm-exec-summary.json'), {
    executionStatus: opts.summary,
  })
}

export function getExecutionDuration (start: [number, number]): number {
  const end = process.hrtime(start)
  return (end[0] * 1e9 + end[1]) / 1e6
}

export type ExecOpts = Required<Pick<ConfigContext, 'selectedProjectsGraph'>> & {
  bail?: boolean
  unsafePerm?: boolean
  reverse?: boolean
  sort?: boolean
  workspaceConcurrency?: number
  shellMode?: boolean
  resumeFrom?: string
  reportSummary?: boolean
  implicitlyFellbackFromRun?: boolean
} & Pick<Config,
| 'bin'
| 'dir'
| 'extraBinPaths'
| 'extraEnv'
| 'lockfileDir'
| 'modulesDir'
| 'nodeOptions'
| 'nodeExperimentalPackageMap'
| 'pnpmHomeDir'
| 'recursive'
| 'reporter'
| 'reporterHidePrefix'
| 'userAgent'
| 'verifyDepsBeforeRun'
| 'workspaceDir'
> & Pick<Config, 'ignoreWorkspaceCycles'> & Pick<ConfigContext, 'cliOptions' | 'allProjectsGraph' | 'prodAllProjectsGraph' | 'prodOnlySelectedProjectDirs'> & CheckDepsStatusOptions

export async function handler (
  opts: ExecOpts,
  params: string[]
): Promise<{ exitCode: number }> {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  if (!params[0]) {
    throw new PnpmError('EXEC_MISSING_COMMAND', '\'pnpm exec\' requires a command to run')
  }
  const limitRun = pLimit(getWorkspaceConcurrency(opts.workspaceConcurrency))

  if (opts.verifyDepsBeforeRun) {
    await runDepsStatusCheck(opts)
  }

  // `exec` runs one command per project, so its task graph is one task per
  // selected project over the project dependency edges: it gets the
  // dependency-order scheduling, while `dependsOn` declarations — which name
  // scripts — do not apply to it.
  const commandName = params[0]
  let taskGraph!: TaskGraph
  if (opts.recursive) {
    const projectDependencies = opts.sort
      ? filteredProjectsDependencies(opts)
      : new Map((Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort().map((project) => [project, [] as ProjectRootDir[]]))
    taskGraph = new Map()
    for (const [project, dependencies] of projectDependencies) {
      taskGraph.set(taskKey(project, commandName), {
        project,
        taskName: commandName,
        scripts: [commandName],
        requested: true,
        dependencies: dependencies.map((dependency) => taskKey(dependency, commandName)),
      })
    }
    if (opts.reverse) {
      taskGraph = reverseTaskGraph(taskGraph)
    }
  } else {
    const project = (opts.cliOptions.dir ?? process.cwd()) as ProjectRootDir
    taskGraph = new Map([[taskKey(project, commandName), {
      project,
      taskName: commandName,
      scripts: [commandName],
      requested: true,
      dependencies: [],
    }]])
    const projectManifest = await tryReadProjectManifest(opts.dir)
    if (projectManifest.manifest != null) {
      opts.selectedProjectsGraph = {
        [opts.dir]: {
          dependencies: [],
          package: {
            ...projectManifest,
            rootDir: opts.dir as ProjectRootDir,
            rootDirRealPath: opts.dir as ProjectRootDirRealPath,
          } as Project,
        },
      }
    }
  }

  if (!opts.selectedProjectsGraph) {
    throw new PnpmError('RECURSIVE_EXEC_NO_PACKAGE', 'No package found in this workspace')
  }

  const fullTaskGraph = taskGraph
  const baseExtraEnv: Record<string, string | undefined> = {
    ...opts.extraEnv,
    ...(opts.nodeOptions ? { NODE_OPTIONS: keepEsmNodePathLoaderOption(opts.nodeOptions, opts.extraEnv?.NODE_OPTIONS) } : {}),
  }
  let taskRunStateContext: TaskRunStateContext | undefined
  if (opts.recursive) {
    taskRunStateContext = new TaskRunStateContext({
      command: 'exec',
      params: [...params, `shell-mode=${Boolean(opts.shellMode)}`],
      settings: taskRunExecutionSettings({ ...opts, extraEnv: baseExtraEnv }),
      graph: fullTaskGraph,
      workspaceDir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
      scriptCommands: () => [],
    })
  }
  if (opts.resumeFrom) {
    const resumeOptions = {
      resumeFrom: opts.resumeFrom,
      selectedProjectsGraph: opts.selectedProjectsGraph,
      taskName: commandName,
    }
    taskGraph = resumeTaskGraphFrom(fullTaskGraph, resumeOptions)
    const completedTasks = await taskRunStateContext?.readCompletedTasks()
    if (completedTasks != null) {
      taskGraph = resumeTaskGraphFrom(fullTaskGraph, { ...resumeOptions, completedTasks })
    }
  }

  // Also the cycle check: a cyclic graph cannot be scheduled, and sequenced
  // into an arbitrary order it would succeed or fail by luck.
  sequenceTasks(taskGraph, {
    workspaceDir: opts.workspaceDir ?? opts.dir,
    ignoreCycles: opts.ignoreWorkspaceCycles,
  })

  let taskRunState: TaskRunState | undefined
  if (taskRunStateContext != null) {
    const initiallyCompleted = new Set<TaskKey>()
    for (const key of fullTaskGraph.keys()) {
      if (!taskGraph.has(key)) initiallyCompleted.add(key)
    }
    taskRunState = await taskRunStateContext.start(initiallyCompleted)
  }

  const result: RecursiveSummary = {}
  for (const node of taskGraph.values()) {
    result[node.project] = { status: 'queued' }
  }
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && existsPnp(opts.workspaceDir)
  const existsPackageMap = existsInDir.bind(null, path.join(opts.modulesDir ?? 'node_modules', '.package-map.json'))
  const workspacePackageMapPath = opts.nodeExperimentalPackageMap && opts.workspaceDir && existsPackageMap(opts.workspaceDir)

  let exitCode = 0
  let firstError: Error | undefined
  let abortError: unknown
  const prependPaths = [
    './node_modules/.bin',
    ...(opts.extraBinPaths ?? []),
  ]
  const reporterShowPrefix = opts.recursive && opts.reporterHidePrefix === false

  const runTask = async (node: TaskNode, key: TaskKey): Promise<TaskCompletion> => {
    try {
      return await runCommandTask(node, key)
    } catch (err: unknown) {
      // An error the per-project handling could not absorb is an
      // infrastructure failure: hold it for rethrow and stop the run.
      abortError ??= err
      return 'aborted'
    }
  }

  const runCommandTask = async (node: TaskNode, key: TaskKey): Promise<TaskCompletion> =>
    limitRun(async (): Promise<TaskCompletion> => {
      // Under --bail a failure stops dispatch, but a task already queued
      // behind the concurrency limit has been dispatched in name only —
      // starting it now would grow the failed run. It stays 'queued'.
      if (opts.bail && firstError != null) {
        return 'passed'
      }
      const prefix = node.project
      result[prefix].status = 'running'
      const startTime = process.hrtime()
      try {
        const pnpPath = workspacePnpPath ?? existsPnp(prefix)
        const packageMapPath = workspacePackageMapPath || (opts.nodeExperimentalPackageMap && existsPackageMap(prefix))
        const extraEnv = { ...baseExtraEnv }
        if (pnpPath) {
          Object.assign(extraEnv, makeNodeRequireOption(pnpPath, extraEnv))
        }
        if (packageMapPath) {
          Object.assign(extraEnv, makeNodePackageMapOption(packageMapPath, extraEnv))
        }
        const env = makeEnv({
          extraEnv: {
            ...extraEnv,
            PNPM_PACKAGE_NAME: opts.selectedProjectsGraph[prefix]?.package.manifest.name,
          },
          prependPaths,
          userAgent: opts.userAgent,
        })
        const [cmd, ...args] = params
        if (reporterShowPrefix) {
          const manifest = await readProjectManifestOnly(prefix)
          const child = trackedExeca(cmd, args, {
            cwd: prefix,
            env,
            stdio: 'pipe',
            shell: opts.shellMode ?? false,
          })
          const lifecycleOpts = {
            wd: prefix,
            depPath: manifest.name ?? path.relative(opts.dir, prefix),
            stage: '(exec)',
          } satisfies Partial<LifecycleMessage>
          // A chunk is neither a line nor a whole number of
          // characters: it may end mid-line, mid-character, or on a
          // newline (which would otherwise report a trailing empty
          // line). A StringDecoder holds back the bytes of a split
          // character, and `pending` holds back a partial line; both
          // are flushed when the stream ends. Only the newly arrived
          // text is scanned for newlines, so a long line costs one
          // concatenation rather than a re-split of everything held.
          const logFn = (stdio: 'stdout' | 'stderr') => {
            const log = (line: string): void => {
              lifecycleLogger.debug({ ...lifecycleOpts, stdio, line })
            }
            const decoder = new StringDecoder('utf8')
            let pending = ''
            const consume = (text: string): void => {
              let start = 0
              for (let end = text.indexOf('\n'); end !== -1; end = text.indexOf('\n', start)) {
                const line = pending + text.slice(start, end)
                pending = ''
                start = end + 1
                // A CRLF terminator contributes no CR to the line, the
                // same as every other line reader in both stacks.
                log(line.endsWith('\r') ? line.slice(0, -1) : line)
              }
              pending += text.slice(start)
            }
            return {
              onData (data: Buffer | string): void {
                consume(typeof data === 'string' ? data : decoder.write(data))
              },
              onEnd (): void {
                consume(decoder.end())
                if (pending !== '') {
                  log(pending)
                  pending = ''
                }
              },
            }
          }
          const stdoutLog = logFn('stdout')
          const stderrLog = logFn('stderr')
          child.stdout!.on('data', stdoutLog.onData)
          child.stderr!.on('data', stderrLog.onData)
          await new Promise<void>((resolve) => {
            void child.once('close', exitCode => {
              stdoutLog.onEnd()
              stderrLog.onEnd()
              lifecycleLogger.debug({
                ...lifecycleOpts,
                exitCode: exitCode ?? 1,
                optional: false,
              })
              resolve()
            })
          })
          await child
        } else {
          const child = trackedExeca(cmd, args, {
            cwd: prefix,
            env,
            stdio: 'inherit',
            shell: opts.shellMode ?? false,
          })
          await child
        }
        result[prefix].status = 'passed'
        result[prefix].duration = getExecutionDuration(startTime)
      } catch (err: any) { // eslint-disable-line
        if (isErrorCommandNotFound(params[0], err, prefix, prependPaths)) {
          err.message = `Command "${params[0]}" not found`
          err.hint = await createExecCommandNotFoundHint(params[0], {
            implicitlyFellbackFromRun: opts.implicitlyFellbackFromRun ?? false,
            dir: opts.dir,
            workspaceDir: opts.workspaceDir,
            modulesDir: opts.modulesDir ?? 'node_modules',
          })
        } else if (!opts.recursive && typeof err.exitCode === 'number') {
          exitCode = err.exitCode
          return 'passed'
        }
        logger.info(err)

        result[prefix] = {
          status: 'failure',
          duration: getExecutionDuration(startTime),
          error: err,
          message: err.message,
          prefix,
        }

        if (opts.bail && firstError == null) {
          if (!err['code']?.startsWith('ERR_PNPM_')) {
            err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          }
          err['prefix'] = prefix
          firstError = err
        }
        return 'failed'
      }
      await taskRunState?.recordPassed(key, node)
      return 'passed'
    })

  try {
    await scheduleTasks(taskGraph, {
      bail: Boolean(opts.bail),
      runTask,
      onTaskSkipped: (node) => {
        result[node.project].status = 'skipped'
      },
    })

    if (abortError !== undefined) {
      throw abortError
    }
    if (firstError != null) {
      if (opts.reportSummary) {
        await writeRecursiveSummary({
          dir: opts.lockfileDir ?? opts.dir,
          summary: result,
        })
      }
      throw firstError
    }

    if (opts.reportSummary) {
      await writeRecursiveSummary({
        dir: opts.lockfileDir ?? opts.dir,
        summary: result,
      })
    }
    throwOnCommandFail('pnpm recursive exec', result)
    await taskRunState?.finish()
    return { exitCode }
  } catch (err: unknown) {
    await taskRunState?.close()
    throw err
  }
}

async function createExecCommandNotFoundHint (
  programName: string,
  opts: {
    dir: string
    implicitlyFellbackFromRun: boolean
    workspaceDir?: string
    modulesDir: string
  }
): Promise<string | undefined> {
  if (opts.implicitlyFellbackFromRun) {
    let nearestScript: string | null | undefined
    try {
      nearestScript = getNearestScript(programName, (await readProjectManifestOnly(opts.dir)).scripts)
    } catch {}
    if (nearestScript) {
      return `Did you mean "pnpm ${nearestScript}"?`
    }
    const nearestProgram = getNearestProgram({
      programName,
      dir: opts.dir,
      workspaceDir: opts.workspaceDir,
      modulesDir: opts.modulesDir,
    })
    if (nearestProgram) {
      return `Did you mean "pnpm ${nearestProgram}"?`
    }
    return undefined
  }
  const nearestProgram = getNearestProgram({
    programName,
    dir: opts.dir,
    workspaceDir: opts.workspaceDir,
    modulesDir: opts.modulesDir,
  })
  if (nearestProgram) {
    return `Did you mean "pnpm exec ${nearestProgram}"?`
  }
  return undefined
}

interface CommandError extends Error {
  originalMessage: string
  shortMessage: string
}

function isErrorCommandNotFound (command: string, error: CommandError, prefix: string, prependPaths: string[]): boolean {
  if (error.originalMessage === `spawn ${command} ENOENT`) {
    return true
  }

  // On Windows, execa 9.x uses cross-spawn only for command parsing (not spawning),
  // so cross-spawn's ENOENT hook never fires. Non-existent commands get wrapped as
  // `cmd.exe /c <command>` which exits with code 1 instead of emitting ENOENT.
  // Fall back to checking if the command exists in PATH, resolving relative paths
  // against the exec prefix to correctly handle --filter contexts.
  if (process.platform === 'win32') {
    const absolutePrependPaths = prependPaths.map(p => path.resolve(prefix, p))
    const { value: searchPath } = prependDirsToPath(absolutePrependPaths)
    return !which.sync(command, { nothrow: true, path: searchPath })
  }

  return false
}
