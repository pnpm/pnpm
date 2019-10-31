import { Config } from '@pnpm/config'
import logger from '@pnpm/logger'
import { ImporterManifest } from '@pnpm/types'
import { render as renderList } from '../list'

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
  if (opts.lockfileDir) {
    console.log(await renderList(pkgs.map((pkg) => pkg.dir), args, {
      ...opts,
      alwaysPrintRootPackage: opts.depth === -1,
      lockfileDir: opts.lockfileDir,
    }, cmd))
    return
  }
  const outputs = []
  for (const { dir } of pkgs) {
    try {
      const output = await renderList([dir], args, {
        ...opts,
        alwaysPrintRootPackage: opts.depth === -1,
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
  if (outputs.length === 0) return

  const joiner = typeof opts.depth === 'number' && opts.depth > -1 ? '\n\n' : '\n'
  const allOutput = outputs.join(joiner)
  console.log(allOutput)
}
