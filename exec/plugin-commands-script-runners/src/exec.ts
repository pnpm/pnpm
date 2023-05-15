import path from 'path'
import { docsUrl, type RecursiveSummary, throwOnCommandFail, readProjectManifestOnly } from '@pnpm/cli-utils'
import { type Config, types } from '@pnpm/config'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import { logger } from '@pnpm/logger'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { sortPackages } from '@pnpm/sort-packages'
import { type Project, type ProjectsGraph } from '@pnpm/types'
import execa from 'execa'
import pLimit from 'p-limit'
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
import { buildCommandNotFoundHint } from './buildCommandNotFoundHint'

export const shorthands = {
  parallel: runShorthands.parallel,
  c: '--shell-mode',
}

export const commandNames = ['exec']

export function rcOptionsTypes () {
  return {
    ...pick([
      'bail',
      'sort',
      'use-node-version',
      'unsafe-perm',
      'workspace-concurrency',
    ], types),
    'shell-mode': Boolean,
    'resume-from': String,
    'report-summary': Boolean,
  }
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
  reverse: Boolean,
})

export function help () {
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
  chunks: string[][]
  selectedProjectsGraph: ProjectsGraph
}) {
  const resumeFromPackagePrefix = Object.keys(selectedProjectsGraph)
    .find((prefix) => selectedProjectsGraph[prefix]?.package.manifest.name === resumeFrom)

  if (!resumeFromPackagePrefix) {
    throw new PnpmError('RESUME_FROM_NOT_FOUND', `Cannot find package ${resumeFrom}. Could not determine where to resume from.`)
  }

  const chunkPosition = chunks.findIndex(chunk => chunk.includes(resumeFromPackagePrefix))
  return chunks.slice(chunkPosition)
}

export async function writeRecursiveSummary (opts: { dir: string, summary: RecursiveSummary }) {
  await writeJsonFile(path.join(opts.dir, 'pnpm-exec-summary.json'), {
    executionStatus: opts.summary,
  })
}

export function createEmptyRecursiveSummary (chunks: string[][]) {
  return chunks.flat().reduce<RecursiveSummary>((acc, prefix) => {
    acc[prefix] = { status: 'queued' }
    return acc
  }, {})
}

export function getExecutionDuration (start: [number, number]) {
  const end = process.hrtime(start)
  return (end[0] * 1e9 + end[1]) / 1e6
}

export async function handler (
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean
    unsafePerm?: boolean
    rawConfig: object
    reverse?: boolean
    sort?: boolean
    workspaceConcurrency?: number
    shellMode?: boolean
    resumeFrom?: string
    reportSummary?: boolean
  } & Pick<Config, 'extraBinPaths' | 'extraEnv' | 'lockfileDir' | 'dir' | 'userAgent' | 'recursive' | 'workspaceDir'>,
  params: string[]
) {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  let chunks!: string[][]
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
  for (const chunk of chunks) {
    await Promise.all(chunk.map(async (prefix: string) =>
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
            },
            prependPaths: [
              './node_modules/.bin',
              ...opts.extraBinPaths,
            ],
            userAgent: opts.userAgent,
          })
          await execa(params[0], params.slice(1), {
            cwd: prefix,
            env,
            stdio: 'inherit',
            shell: opts.shellMode ?? false,
          })
          result[prefix].status = 'passed'
          result[prefix].duration = getExecutionDuration(startTime)
        } catch (err: any) { // eslint-disable-line
          if (await isErrorCommandNotFound(params[0], err)) {
            err.hint = buildCommandNotFoundHint(params[0], (await readProjectManifestOnly(opts.dir)).scripts)
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
          /* eslint-enable @typescript-eslint/dot-notation */
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

interface CommandError extends Error {
  originalMessage: string
  shortMessage: string
}

async function isErrorCommandNotFound (command: string, error: CommandError) {
  // Mac/Linux
  if (error.originalMessage === `spawn ${command} ENOENT`) {
    return true
  }

  // Windows
  return error.shortMessage === `Command failed with exit code 1: ${command}` &&
    !(await which(command, { nothrow: true }))
}
