import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import execa = require('execa')
import pLimit = require('p-limit')
import dividePackagesToChunks from './dividePackagesToChunks'
import RecursiveSummary from './recursiveSummary'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    bail: boolean,
    concurrency: number,
    unsafePerm: boolean,
    rawNpmConfig: object,
  },
): Promise<RecursiveSummary> => {
  const {chunks} = dividePackagesToChunks(pkgs)

  const limitRun = pLimit(opts.concurrency)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        try {
          await execa(args[0], args.slice(1), {cwd: prefix, stdio: 'inherit'})
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

  return result
}
