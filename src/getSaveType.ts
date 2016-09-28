import {PnpmOptions} from './types'
export type DependenciesType = 'dependencies' | 'devDependencies' | 'optionalDependencies'

export default function getSaveType (opts: PnpmOptions): DependenciesType | null {
  if (opts.save || opts.global) return 'dependencies'
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  return null
}
