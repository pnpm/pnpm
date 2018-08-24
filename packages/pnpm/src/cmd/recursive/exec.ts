import logger from '@pnpm/logger'
import execa = require('execa')
import pLimit = require('p-limit')
import RecursiveSummary from './recursiveSummary'

export default async (
  packageChunks: string[][],
  args: string[],
  cmd: string,
  opts: {
    bail: boolean,
    workspaceConcurrency: number,
    unsafePerm: boolean,
    rawNpmConfig: object,
  },
): Promise<RecursiveSummary> => {
  const limitRun = pLimit(opts.workspaceConcurrency)

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  for (const chunk of packageChunks) {
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
