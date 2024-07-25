import assert from 'assert'
import util from 'util'
import { type Config } from '@pnpm/config'
import { logger } from '@pnpm/logger'
import { type IncludedDependencies, type Project } from '@pnpm/types'
import { render } from './list'

export async function listRecursive (
  pkgs: Project[],
  params: string[],
  opts: Pick<Config, 'lockfileDir' | 'virtualStoreDirMaxLength'> & {
    depth?: number
    include: IncludedDependencies
    long?: boolean
    parseable?: boolean
    lockfileDir?: string
  }
): Promise<string> {
  const depth = opts.depth ?? 0
  if (opts.lockfileDir) {
    return render(pkgs.map((pkg) => pkg.rootDir), params, {
      ...opts,
      alwaysPrintRootPackage: depth === -1,
      lockfileDir: opts.lockfileDir,
    })
  }
  const outputs = (await Promise.all(pkgs.map(async ({ rootDir }) => {
    try {
      return await render([rootDir], params, {
        ...opts,
        alwaysPrintRootPackage: depth === -1,
        lockfileDir: opts.lockfileDir ?? rootDir,
      })
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      const errWithPrefix = Object.assign(err, {
        prefix: rootDir,
      })
      logger.info(errWithPrefix)
      throw errWithPrefix
    }
  }))).filter(Boolean)
  if (outputs.length === 0) return ''

  const joiner = typeof depth === 'number' && depth > -1 ? '\n\n' : '\n'
  return outputs.join(joiner)
}
