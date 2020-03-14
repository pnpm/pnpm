import { RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, types } from '@pnpm/config'
import logger from '@pnpm/logger'
import sortPackages from '@pnpm/sort-packages'
import execa = require('execa')
import pLimit from 'p-limit'
import R = require('ramda')
import renderHelp = require('render-help')

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
    usages: ['-r exec -- <command> [args...]'],
  })
}

export async function handler (
  opts: Required<Pick<Config, 'selectedProjectsGraph'>> & {
    bail?: boolean,
    unsafePerm?: boolean,
    rawConfig: object,
    sort?: boolean,
    workspaceConcurrency?: number,
  },
  params: string[],
) {
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const chunks = opts.sort
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        try {
          await execa(params[0], params.slice(1), {
            cwd: prefix,
            env: {
              ...process.env,
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

          // tslint:disable:no-string-literal
          err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL'
          err['prefix'] = prefix
          // tslint:enable:no-string-literal
          throw err
        }
      },
    )))
  }

  throwOnCommandFail('pnpm recursive exec', result)
}
