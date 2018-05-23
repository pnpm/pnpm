import {PackageJson} from '@pnpm/types'
import list from '../list'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    depth?: number,
    only?: 'dev' | 'prod',
    long?: boolean,
    parseable?: boolean,
    global: boolean,
    independentLeaves: boolean,
  },
) => {
  for (const pkg of pkgs) {
    await list(args, {...opts, prefix: pkg.path, alwaysPrintRootPackage: false}, cmd)
  }
}
