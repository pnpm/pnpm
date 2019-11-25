import { Config } from '@pnpm/config'
import logger from '@pnpm/logger'
import { list } from '@pnpm/plugin-commands-listing'
import { ImporterManifest } from '@pnpm/types'

export default async (
  pkgs: Array<{ dir: string, manifest: ImporterManifest }>,
  args: string[],
  cmd: string,
  opts: Config & {
    depth?: number,
    long?: boolean,
    parseable?: boolean,
    lockfileDir?: string,
  },
) => {
  const depth = opts.depth ?? 0
  if (opts.lockfileDir) {
    return list.render(pkgs.map((pkg) => pkg.dir), args, {
      ...opts,
      alwaysPrintRootPackage: depth === -1,
      lockfileDir: opts.lockfileDir,
    }, cmd)
  }
  const outputs = []
  for (const { dir } of pkgs) {
    try {
      const output = await list.render([dir], args, {
        ...opts,
        alwaysPrintRootPackage: depth === -1,
        lockfileDir: opts.lockfileDir || dir,
      }, cmd)
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
