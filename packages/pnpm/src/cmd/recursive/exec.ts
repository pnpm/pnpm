import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import execa = require('execa')
import pLimit = require('p-limit')
import dividePackagesToChunks from './dividePackagesToChunks'

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
) => {
  const {chunks} = dividePackagesToChunks(pkgs)

  const limitRun = pLimit(opts.concurrency)
  let failed = false

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        try {
          await execa(args[0], args.slice(1), {cwd: prefix, stdio: 'inherit'})
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            failed = true
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

  if (failed) {
    const err = new Error('exec failed')
    err['code'] = 'ERR_PNPM_RECURSIVE_EXEC_FAIL' // tslint:disable-line:no-string-literal
    throw err
  }
}
