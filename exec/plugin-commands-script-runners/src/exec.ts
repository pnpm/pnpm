import path from 'node:path'

import execa from 'execa'
import which from 'which'
import pLimit from 'p-limit'
import PATH from 'path-name'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import writeJsonFile from 'write-json-file'

import {
  docsUrl,
  throwOnCommandFail,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import { PnpmError } from '@pnpm/error'
import { types } from '@pnpm/config'
import { sortPackages } from '@pnpm/sort-packages'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import type { Project, ProjectsGraph, Config, RecursiveSummary, CommandError } from '@pnpm/types'

import {
  PARALLEL_OPTION_HELP,
  RESUME_FROM_OPTION_HELP,
  REPORT_SUMMARY_OPTION_HELP,
  shorthands as runShorthands,
} from './run'
import { makeEnv } from './makeEnv'
import { existsInDir } from './existsInDir'
import { getNearestProgram, getNearestScript } from './buildCommandNotFoundHint'

export const shorthands = {
  parallel: runShorthands.parallel,
  c: '--shell-mode',
}

export const commandNames = ['exec']

export function rcOptionsTypes(): {
  'shell-mode': BooleanConstructor;
  'resume-from': StringConstructor;
  'report-summary': BooleanConstructor;
  bail: BooleanConstructor;
  sort: BooleanConstructor;
  'use-node-version': StringConstructor;
  'unsafe-perm': BooleanConstructor;
  'workspace-concurrency': NumberConstructor;
} {
  return {
    ...pick(
      [
        'bail',
        'sort',
        'use-node-version',
        'unsafe-perm',
        'workspace-concurrency',
      ],
      types
    ),
    'shell-mode': Boolean,
    'resume-from': String,
    'report-summary': Boolean,
  }
}

export function cliOptionsTypes(): {
  recursive: BooleanConstructor;
  reverse: BooleanConstructor;
  'shell-mode': BooleanConstructor;
  'resume-from': StringConstructor;
  'report-summary': BooleanConstructor;
  bail: BooleanConstructor;
  sort: BooleanConstructor;
  'use-node-version': StringConstructor;
  'unsafe-perm': BooleanConstructor;
  'workspace-concurrency': NumberConstructor;
} {
  return {
    ...rcOptionsTypes(),
    recursive: Boolean,
    reverse: Boolean,
  };
}

export function help(): string {
  return renderHelp({
    description: 'Run a shell command in the context of a project.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          PARALLEL_OPTION_HELP,
          {
            description:
              'Run the shell command in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description:
              'If exist, runs file inside of a shell. \
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

export function getResumedPackageChunks({
  resumeFrom,
  chunks,
  selectedProjectsGraph,
}: {
  resumeFrom: string
  chunks: string[][]
  selectedProjectsGraph: ProjectsGraph
}) {
  const resumeFromPackagePrefix = Object.keys(selectedProjectsGraph).find(
    (prefix: string): boolean => {
      return selectedProjectsGraph[prefix]?.package.manifest?.name === resumeFrom;
    }
  )

  if (!resumeFromPackagePrefix) {
    throw new PnpmError(
      'RESUME_FROM_NOT_FOUND',
      `Cannot find package ${resumeFrom}. Could not determine where to resume from.`
    )
  }

  const chunkPosition = chunks.findIndex((chunk) =>
    chunk.includes(resumeFromPackagePrefix)
  )

  return chunks.slice(chunkPosition)
}

export async function writeRecursiveSummary(opts: {
  dir: string
  summary: RecursiveSummary
}): Promise<void> {
  await writeJsonFile(path.join(opts.dir, 'pnpm-exec-summary.json'), {
    executionStatus: opts.summary,
  })
}

export function createEmptyRecursiveSummary(
  chunks: string[][]
): RecursiveSummary {
  return chunks
    .flat()
    .reduce<RecursiveSummary>((acc: RecursiveSummary, prefix: string): RecursiveSummary => {
    acc[prefix] = { status: 'queued' }
    return acc
  }, {})
}

export function getExecutionDuration(start: [number, number]): number {
  const end = process.hrtime(start)

  return (end[0] * 1e9 + end[1]) / 1e6
}

export async function handler(
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean | undefined
    unsafePerm?: boolean | undefined
    rawConfig: object
    reverse?: boolean | undefined
    sort?: boolean | undefined
    workspaceConcurrency?: number | undefined
    shellMode?: boolean | undefined
    resumeFrom?: string | undefined
    reportSummary?: boolean | undefined
    implicitlyFellbackFromRun?: boolean | undefined
  } & Pick<
    Config,
      | 'extraBinPaths'
      | 'extraEnv'
      | 'lockfileDir'
      | 'modulesDir'
      | 'dir'
      | 'userAgent'
      | 'recursive'
      | 'workspaceDir'
  >,
  params: string[]
): Promise<{
    exitCode: number;
  }> {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  let chunks: string[][]

  if (opts.recursive) {
    chunks = opts.sort
      ? sortPackages(opts.selectedProjectsGraph)
      : [Object.keys(opts.selectedProjectsGraph).sort()]

    if (opts.reverse) {
      chunks = chunks.reverse()
    }
  } else {
    chunks = [[opts.dir]]

    const project = await tryReadProjectManifest(opts.dir)

    if (project.manifest != null) {
      opts.selectedProjectsGraph = {
        [opts.dir]: {
          dependencies: [],
          package: {
            ...project,
            dir: opts.dir,
            modulesDir: '',
            id: '',
            rootDir: '',
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

  const workspacePnpPath =
    opts.workspaceDir && (await existsPnp(opts.workspaceDir))

  let exitCode = 0

  const prependPaths = ['./node_modules/.bin', ...opts.extraBinPaths]

  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop
    await Promise.all(
      chunk.map(async (prefix: string) =>
        limitRun(async () => {
          result[prefix].status = 'running'

          const startTime = process.hrtime()

          try {
            const pnpPath = workspacePnpPath ?? (await existsPnp(prefix))

            const extraEnv = {
              ...opts.extraEnv,
              ...(pnpPath ? makeNodeRequireOption(pnpPath) : {}),
            }

            const env = makeEnv({
              extraEnv: {
                ...extraEnv,
                PNPM_PACKAGE_NAME:
                  opts.selectedProjectsGraph[prefix]?.package.manifest?.name,
              },
              prependPaths,
              userAgent: opts.userAgent,
            })

            await execa(params[0], params.slice(1), {
              cwd: prefix,
              env,
              stdio: 'inherit',
              shell: opts.shellMode ?? false,
            })

            result[prefix].status = 'passed'

            // Add the 'duration' property to the 'Actions' type and the 'ActionQueued' type
            // @ts-ignore
            result[prefix].duration = getExecutionDuration(startTime) as number
          } catch (err: unknown) {
            // @ts-ignore
            if (err && isErrorCommandNotFound(params[0], err, prependPaths)) {
              // @ts-ignore
              err.message = `Command "${params[0]}" not found`

              // @ts-ignore
              err.hint = await createExecCommandNotFoundHint(params[0], {
                implicitlyFellbackFromRun:
                  opts.implicitlyFellbackFromRun ?? false,
                dir: opts.dir,
                workspaceDir: opts.workspaceDir,
                modulesDir: opts.modulesDir ?? 'node_modules',
              })

              // @ts-ignore
            } else if (!opts.recursive && typeof err.exitCode === 'number') {
              // @ts-ignore
              exitCode = err.exitCode

              return
            }

            // @ts-ignore
            logger.info(err)

            result[prefix] = {
              status: 'failure',
              duration: getExecutionDuration(startTime),
              // @ts-ignore
              error: err,
              // @ts-ignore
              message: err.message,
              prefix,
            }

            if (!opts.bail) {
              return
            }

            // @ts-ignore
            if (!err.code?.startsWith('ERR_PNPM_')) {
              // @ts-ignore
              err.code = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
            }

            // @ts-ignore
            err.prefix = prefix

            // @ts-ignore
            if (opts.reportSummary) {
              await writeRecursiveSummary({
                dir: opts.lockfileDir ?? opts.dir,
                summary: result,
              })
            }

            throw err
          }
        })
      )
    )
  }

  if (opts.reportSummary) {
    await writeRecursiveSummary({
      dir: opts.lockfileDir ?? opts.dir,
      summary: result,
    })
  }

  throwOnCommandFail('pnpm recursive exec', result)

  return { exitCode }
}

async function createExecCommandNotFoundHint(
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
      nearestScript = getNearestScript(
        programName,
        (await readProjectManifestOnly(opts.dir)).scripts
      )
    } catch (_err: unknown) {}

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

function isErrorCommandNotFound(
  command: string,
  error: CommandError,
  prependPaths: string[]
): boolean {
  // Mac/Linux
  if (process.platform === 'linux' || process.platform === 'darwin') {
    return error.originalMessage === `spawn ${command} ENOENT`
  }

  // Windows
  if (process.platform === 'win32') {
    const prepend = prependPaths.join(path.delimiter)
    const whichPath = process.env[PATH]
      ? `${prepend}${path.delimiter}${process.env[PATH] as string}`
      : prepend
    return !which.sync(command, {
      nothrow: true,
      path: whichPath,
    })
  }

  return false
}
