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
    concurrency: number,
    unsafePerm: boolean,
    rawNpmConfig: object,
  },
) => {
  const {chunks} = dividePackagesToChunks(pkgs)

  const limitRun = pLimit(opts.concurrency)

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitRun(async () => {
        try {
          await execa(args[0], args.slice(1), {cwd: prefix, stdio: 'inherit'})
        } catch (err) {
          logger.info(err)
          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          throw err
        }
      },
    )))
  }
}
