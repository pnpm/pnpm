import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import list from '../list'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    depth?: number,
    production: boolean,
    development: boolean,
    long?: boolean,
    parseable?: boolean,
    global: boolean,
    independentLeaves: boolean,
  },
) => {
  for (const pkg of pkgs) {
    try {
      await list(args, {...opts, prefix: pkg.path, alwaysPrintRootPackage: false}, cmd)
    } catch (err) {
      logger.info(err)
      err['prefix'] = pkg.path // tslint:disable-line:no-string-literal
      throw err
    }
  }
}
