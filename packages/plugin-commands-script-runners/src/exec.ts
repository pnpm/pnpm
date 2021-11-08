import path from 'path'
import { docsUrl, RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, types } from '@pnpm/config'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import { tryReadProjectManifest } from '@pnpm/read-project-manifest'
import sortPackages from '@pnpm/sort-packages'
import { Project } from '@pnpm/types'
import execa from 'execa'
import pLimit from 'p-limit'
import PATH from 'path-name'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import existsInDir from './existsInDir'
import {
  PARALLEL_OPTION_HELP,
  shorthands as runShorthands,
} from './run'

export const shorthands = {
  parallel: runShorthands.parallel,
}

export const commandNames = ['exec']

export function rcOptionsTypes () {
  return pick([
    'bail',
    'sort',
    'unsafe-perm',
    'workspace-concurrency',
  ], types)
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
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
        ],
      },
    ],
    url: docsUrl('exec'),
    usages: ['pnpm [-r] exec <command> [args...]'],
  })
}

export async function handler (
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean
    unsafePerm?: boolean
    rawConfig: object
    sort?: boolean
    workspaceConcurrency?: number
  } & Pick<Config, 'extraBinPaths' | 'lockfileDir' | 'dir' | 'recursive' | 'workspaceDir'>,
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
  const existsPnp = existsInDir.bind(null, '.pnp.cjs')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  for (const chunk of chunks) {
    await Promise.all(chunk.map(async (prefix: string) =>
      limitRun(async () => {
        try {
          const pnpPath = workspacePnpPath ?? await existsPnp(prefix)
          const extraEnv = pnpPath
            ? makeNodeRequireOption(pnpPath)
            : {}
          await execa(params[0], params.slice(1), {
            cwd: prefix,
            env: {
              ...process.env,
              ...extraEnv,
              [PATH]: [
                ...opts.extraBinPaths,
                path.join(opts.dir, 'node_modules/.bin'),
                process.env[PATH],
              ].join(path.delimiter),
              PNPM_PACKAGE_NAME: opts.selectedProjectsGraph?.[prefix]?.package.manifest.name,
            },
            stdio: 'inherit',
          })
          result.passes++
        } catch (err: any) { // eslint-disable-line
          if (!opts.recursive && typeof err.exitCode === 'number') {
            return {
              exitCode: err.exitCode,
            }
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

          /* eslint-disable @typescript-eslint/dot-notation */
          err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          err['prefix'] = prefix
          /* eslint-enable @typescript-eslint/dot-notation */
          throw err
        }
        return { exitCode: 0 }
      }
      )))
  }

  throwOnCommandFail('pnpm recursive exec', result)
}
