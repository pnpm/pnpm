import { Config, Project } from '@pnpm/config'
import logger from '@pnpm/logger'
import { render } from './list'

export default async (
  pkgs: Project[],
  args: string[],
  opts: Pick<Config, 'lockfileDir' | 'include'> & {
    depth?: number,
    long?: boolean,
    parseable?: boolean,
    lockfileDir?: string,
  },
) => {
  const depth = opts.depth ?? 0
  if (opts.lockfileDir) {
    return render(pkgs.map((pkg) => pkg.dir), args, {
      ...opts,
      alwaysPrintRootPackage: depth === -1,
      lockfileDir: opts.lockfileDir,
    })
  }
  const outputs = []
  for (const { dir } of pkgs) {
    try {
      const output = await render([dir], args, {
        ...opts,
        alwaysPrintRootPackage: depth === -1,
        lockfileDir: opts.lockfileDir || dir,
      })
      if (!output) continue
      outputs.push(output)
    } catch (err) {
      logger.info(err)
      err['prefix'] = dir // tslint:disable-line:no-string-literal
      throw err
    }
  }
  if (outputs.length === 0) return ''

  const joiner = typeof depth === 'number' && depth > -1 ? '\n\n' : '\n'
  return outputs.join(joiner)
}
