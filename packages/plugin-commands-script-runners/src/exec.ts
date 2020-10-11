import { RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, types } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { makeNodeRequireOption } from '@pnpm/lifecycle'
import logger from '@pnpm/logger'
import sortPackages from '@pnpm/sort-packages'
import existsInDir from './existsInDir'
import {
  PARALLEL_OPTION_HELP,
  shorthands as runShorthands,
} from './run'
import execa = require('execa')
import pLimit = require('p-limit')
import R = require('ramda')
import renderHelp = require('render-help')

export const shorthands = {
  parallel: runShorthands.parallel,
}

export const commandNames = ['exec']

export function rcOptionsTypes () {
  return R.pick([
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
    description: 'Run a command in each package.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          PARALLEL_OPTION_HELP,
        ],
      },
    ],
    usages: ['-r exec -- <command> [args...]'],
  })
}

export async function handler (
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean
    unsafePerm?: boolean
    rawConfig: object
    sort?: boolean
    workspaceConcurrency?: number
  } & Pick<Config, 'recursive' | 'workspaceDir'>,
  params: string[]
) {
  if (!opts.recursive) {
    throw new PnpmError('EXEC_NOT_RECURSIVE', 'The "pnpm exec" command currently only works with the "-r" option')
  }
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const chunks = opts.sort
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]
  const existsPnp = existsInDir.bind(null, '.pnp.js')
  const workspacePnpPath = opts.workspaceDir && await existsPnp(opts.workspaceDir)

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
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
              PNPM_PACKAGE_NAME: opts.selectedProjectsGraph[prefix].package.manifest.name,
            },
            stdio: 'inherit',
          })
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

          /* eslint-disable @typescript-eslint/dot-notation */
          err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          err['prefix'] = prefix
          /* eslint-enable @typescript-eslint/dot-notation */
          throw err
        }
      }
      )))
  }

  throwOnCommandFail('pnpm recursive exec', result)
}
