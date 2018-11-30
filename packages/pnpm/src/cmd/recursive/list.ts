import logger from '@pnpm/logger'
import { PackageJson } from '@pnpm/types'
import { render as renderList } from '../list'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    depth?: number,
    development: boolean,
    long?: boolean,
    parseable?: boolean,
    production: boolean,
    shrinkwrapDirectory?: string,
  },
) => {
  const outputs = []
  for (const pkg of pkgs) {
    try {
      const output = await renderList(args, { ...opts, prefix: pkg.path, alwaysPrintRootPackage: opts.depth === -1 }, cmd)
      if (!output) continue
      outputs.push(output)
    } catch (err) {
      logger.info(err)
      err['prefix'] = pkg.path // tslint:disable-line:no-string-literal
      throw err
    }
  }
  if (outputs.length === 0) return

  const joiner = opts.depth && opts.depth > -1 ? '\n\n' : '\n'
  const allOutput = outputs.join(joiner)
  console.log(allOutput)
}
