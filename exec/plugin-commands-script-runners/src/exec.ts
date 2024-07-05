import path from 'path'
import { docsUrl, type RecursiveSummary, throwOnCommandFail, readProjectManifestOnly } from '@pnpm/cli-utils'
import { type LifecycleMessage, lifecycleLogger } from '@pnpm/core-loggers'
import { type Config, types } from '@pnpm/config'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import { logger } from '@pnpm/logger'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { sortPackages } from '@pnpm/sort-packages'
import { type Project, type ProjectsGraph, type ProjectRootDir, type ProjectRootDirRealPath } from '@pnpm/types'
import execa from 'execa'
import pLimit from 'p-limit'
import PATH from 'path-name'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { existsInDir } from './existsInDir'
import { makeEnv } from './makeEnv'
import {
  PARALLEL_OPTION_HELP,
  REPORT_SUMMARY_OPTION_HELP,
  RESUME_FROM_OPTION_HELP,
  shorthands as runShorthands,
} from './run'
import { PnpmError } from '@pnpm/error'
import which from 'which'
import writeJsonFile from 'write-json-file'
import { getNearestProgram, getNearestScript } from './buildCommandNotFoundHint'

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
      'use-node-version',
      'unsafe-perm',
      'workspace-concurrency',
      'reporter-hide-prefix',
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
        ],
      },
    ],
    url: docsUrl('exec'),
    usages: ['pnpm [-r] [-c] exec <command> [args...]'],
  })
}

export function getResumedPackageChunks ({
  resumeFrom,
  chunks,
  selectedProjectsGraph,
}: {
  resumeFrom: string
  chunks: ProjectRootDir[][]
  selectedProjectsGraph: ProjectsGraph
}): ProjectRootDir[][] {
  const resumeFromPackagePrefix = (Object.keys(selectedProjectsGraph) as ProjectRootDir[])
    .find((prefix) => selectedProjectsGraph[prefix]?.package.manifest.name === resumeFrom)

  if (!resumeFromPackagePrefix) {
    throw new PnpmError('RESUME_FROM_NOT_FOUND', `Cannot find package ${resumeFrom}. Could not determine where to resume from.`)
  }

  const chunkPosition = chunks.findIndex(chunk => chunk.includes(resumeFromPackagePrefix))
  return chunks.slice(chunkPosition)
}

export async function writeRecursiveSummary (opts: { dir: string, summary: RecursiveSummary }): Promise<void> {
  await writeJsonFile(path.join(opts.dir, 'pnpm-exec-summary.json'), {
    executionStatus: opts.summary,
  })
}

export function createEmptyRecursiveSummary (chunks: string[][]): RecursiveSummary {
  return chunks.flat().reduce<RecursiveSummary>((acc, prefix) => {
    acc[prefix] = { status: 'queued' }
    return acc
  }, {})
}

export function getExecutionDuration (start: [number, number]): number {
  const end = process.hrtime(start)
  return (end[0] * 1e9 + end[1]) / 1e6
}

export async function handler (
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean
    unsafePerm?: boolean
    reverse?: boolean
    sort?: boolean
    workspaceConcurrency?: number
    shellMode?: boolean
    resumeFrom?: string
    reportSummary?: boolean
    implicitlyFellbackFromRun?: boolean
  } & Pick<Config, 'extraBinPaths' | 'extraEnv' | 'lockfileDir' | 'modulesDir' | 'dir' | 'userAgent' | 'recursive' | 'reporterHidePrefix' | 'workspaceDir' | 'nodeOptions'>,
  params: string[]
): Promise<{ exitCode: number }> {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  let chunks!: ProjectRootDir[][]
  if (opts.recursive) {
    chunks = opts.sort
      ? sortPackages(opts.selectedProjectsGraph)
      : [(Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort()]
    if (opts.reverse) {
      chunks = chunks.reverse()
    }
  } else {
    chunks = [[opts.dir as ProjectRootDir]]
    const project = await tryReadProjectManifest(opts.dir)
    if (project.manifest != null) {
      opts.selectedProjectsGraph = {
        [opts.dir]: {
          dependencies: [],
          package: {
            ...project,
            rootDir: opts.dir as ProjectRootDir,
            rootDirRealPath: opts.dir as ProjectRootDirRealPath,
          } as Project,
        },
      }
    }
  }

  if (opts.resumeFrom) {
    chunks = getResumedPackageChunks({
      resumeFrom: opts.resumeFrom,
      chunks,
      selectedProjectsGraph: opts.selectedProjectsGraph,
    })
  }

  const result = createEmptyRecursiveSummary(chunks)
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  let exitCode = 0
  const prependPaths = [
    './node_modules/.bin',
    ...opts.extraBinPaths,
  ]
  const reporterShowPrefix = opts.recursive && opts.reporterHidePrefix === false
  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop
    await Promise.all(chunk.map(async (prefix) =>
      limitRun(async () => {
        result[prefix].status = 'running'
        const startTime = process.hrtime()
        try {
          const pnpPath = workspacePnpPath ?? await existsPnp(prefix)
          const extraEnv = {
            ...opts.extraEnv,
            ...(pnpPath ? makeNodeRequireOption(pnpPath) : {}),
          }
          const env = makeEnv({
            extraEnv: {
              ...extraEnv,
              PNPM_PACKAGE_NAME: opts.selectedProjectsGraph[prefix]?.package.manifest.name,
              ...(opts.nodeOptions ? { NODE_OPTIONS: opts.nodeOptions } : {}),
            },
            prependPaths,
            userAgent: opts.userAgent,
          })
          const [cmd, ...args] = params
          if (reporterShowPrefix) {
            const manifest = await readProjectManifestOnly(prefix)
            const child = execa(cmd, args, {
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
            const logFn = (stdio: 'stdout' | 'stderr') => (data: unknown): void => {
              for (const line of String(data).split('\n')) {
                lifecycleLogger.debug({
                  ...lifecycleOpts,
                  stdio,
                  line,
                })
              }
            }
            child.stdout!.on('data', logFn('stdout'))
            child.stderr!.on('data', logFn('stderr'))
            void child.once('close', exitCode => {
              lifecycleLogger.debug({
                ...lifecycleOpts,
                exitCode: exitCode ?? 1,
                optional: false,
              })
            })
            await child
          } else {
            await execa(cmd, args, {
              cwd: prefix,
              env,
              stdio: 'inherit',
              shell: opts.shellMode ?? false,
            })
          }
          result[prefix].status = 'passed'
          result[prefix].duration = getExecutionDuration(startTime)
        } catch (err: any) { // eslint-disable-line
          if (isErrorCommandNotFound(params[0], err, prependPaths)) {
            err.message = `Command "${params[0]}" not found`
            err.hint = await createExecCommandNotFoundHint(params[0], {
              implicitlyFellbackFromRun: opts.implicitlyFellbackFromRun ?? false,
              dir: opts.dir,
              workspaceDir: opts.workspaceDir,
              modulesDir: opts.modulesDir ?? 'node_modules',
            })
          } else if (!opts.recursive && typeof err.exitCode === 'number') {
            exitCode = err.exitCode
            return
          }
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

          if (!err['code']?.startsWith('ERR_PNPM_')) {
            err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          }
          err['prefix'] = prefix
          opts.reportSummary && await writeRecursiveSummary({
            dir: opts.lockfileDir ?? opts.dir,
            summary: result,
          })

          throw err
        }
      }
      )))
  }

  opts.reportSummary && await writeRecursiveSummary({
    dir: opts.lockfileDir ?? opts.dir,
    summary: result,
  })
  throwOnCommandFail('pnpm recursive exec', result)
  return { exitCode }
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
    } catch (_err) {}
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

function isErrorCommandNotFound (command: string, error: CommandError, prependPaths: string[]): boolean {
  // Mac/Linux
  if (process.platform === 'linux' || process.platform === 'darwin') {
    return error.originalMessage === `spawn ${command} ENOENT`
  }

  // Windows
  if (process.platform === 'win32') {
    const prepend = prependPaths.join(path.delimiter)
    const whichPath = process.env[PATH] ? `${prepend}${path.delimiter}${process.env[PATH] as string}` : prepend
    return !which.sync(command, {
      nothrow: true,
      path: whichPath,
    })
  }

  return false
}
