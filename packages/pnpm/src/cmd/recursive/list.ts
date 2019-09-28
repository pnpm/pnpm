import { Config } from '@pnpm/config'
import logger from '@pnpm/logger'
import { PackageJson } from '@pnpm/types'
import { render as renderList } from '../list'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: Config & {
    depth?: number,
    long?: boolean,
    parseable?: boolean,
    lockfileDirectory?: string,
  },
) => {
  if (opts.lockfileDirectory) {
    console.log(await renderList(pkgs.map((pkg) => pkg.path), args, {
      ...opts,
      alwaysPrintRootPackage: opts.depth === -1,
      lockfileDirectory: opts.lockfileDirectory,
    }, cmd))
    return
  }
  const outputs = []
  for (const { path } of pkgs) {
    try {
      const output = await renderList([path], args, {
        ...opts,
        alwaysPrintRootPackage: opts.depth === -1,
        lockfileDirectory: opts.lockfileDirectory || path,
      }, cmd)
      if (!output) continue
      outputs.push(output)
    } catch (err) {
      logger.info(err)
      err['prefix'] = path // tslint:disable-line:no-string-literal
      throw err
    }
  }
  if (outputs.length === 0) return

  const joiner = typeof opts.depth === 'number' && opts.depth > -1 ? '\n\n' : '\n'
  const allOutput = outputs.join(joiner)
  console.log(allOutput)
}
