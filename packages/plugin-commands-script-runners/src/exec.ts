import { RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { Config, types, WsPkgsGraph } from '@pnpm/config'
import logger from '@pnpm/logger'
import sortPackages from '@pnpm/sort-packages'
import execa = require('execa')
import pLimit from 'p-limit'
import R = require('ramda')
import renderHelp = require('render-help')

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
    'bail',
    'unsafe-perm',
    'workspace-concurrency',
  ], types)
}

export function help () {
  return renderHelp({
    description: 'Run a command in each package.',
    usages: ['-r exec -- <command> [args...]'],
  })
}

export async function handler (
  args: string[],
  opts: Required<Pick<Config, 'selectedWsPkgsGraph'>> & {
    bail?: boolean,
    unsafePerm?: boolean,
    rawConfig: object,
    sort?: boolean,
    workspaceConcurrency?: number,
  },
) {
  const limitRun = pLimit(opts.workspaceConcurrency ?? 4)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const chunks = opts.sort
    ? sortPackages(opts.selectedWsPkgsGraph)
    : [Object.keys(opts.selectedWsPkgsGraph).sort()]

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        try {
          await execa(args[0], args.slice(1), {
            cwd: prefix,
            env: {
              ...process.env,
              PNPM_PACKAGE_NAME: opts.selectedWsPkgsGraph[prefix].package.manifest.name,
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
