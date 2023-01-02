import path from 'path'
import { docsUrl, RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, types } from '@pnpm/config'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import { logger } from '@pnpm/logger'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import { sortPackages } from '@pnpm/sort-packages'
import { Project, ProjectsGraph } from '@pnpm/types'
import execa from 'execa'
import pLimit from 'p-limit'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { existsInDir } from './existsInDir'
import { makeEnv } from './makeEnv'
import {
  PARALLEL_OPTION_HELP,
  shorthands as runShorthands,
} from './run'
import { PnpmError } from '@pnpm/error'

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
          {
            description: 'command executed from given package',
            name: '--resume-from',
          },
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
  } & Pick<Config, 'extraBinPaths' | 'extraEnv' | 'lockfileDir' | 'dir' | 'userAgent' | 'recursive' | 'workspaceDir'>,
  params: string[]
) {
  // For backward compatibility
  if (params[0] === '--') {
    params.shift()
  }
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

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

  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  let exitCode = 0
  for (const chunk of chunks) {
    await Promise.all(chunk.map(async (prefix: string) =>
      limitRun(async () => {
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
              path.join(prefix, 'node_modules/.bin'),
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
          result.passes++
        } catch (err: any) { // eslint-disable-line
          if (!opts.recursive && typeof err.exitCode === 'number') {
            exitCode = err.exitCode
            return
          }
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix,
            })
            return
          }

          err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          err['prefix'] = prefix
          /* eslint-enable @typescript-eslint/dot-notation */
          throw err
        }
      }
      )))
  }

  throwOnCommandFail('pnpm recursive exec', result)
  return { exitCode }
}
