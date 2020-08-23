import { Config } from '@pnpm/config'
import { DependenciesField } from '@pnpm/types'

export default function getSaveType (
  opts: Pick<Config, 'saveDev' | 'saveOptional' | 'saveProd' | 'savePeer'>
): DependenciesField | undefined {
  if (opts.saveDev === true || opts.savePeer) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  if (opts.saveProd) return 'dependencies'
  return undefined
}
