import {PnpmOptions} from './types'
export type DependenciesType = 'dependencies' | 'devDependencies' | 'optionalDependencies'

export default function getSaveType (opts: PnpmOptions): DependenciesType {
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  return 'dependencies'
}
