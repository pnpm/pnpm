import {PnpmOptions} from './types'
export type DependenciesType = 'dependencies' | 'devDependencies' | 'optionalDependencies'

export const dependenciesTypes: DependenciesType[] = ['dependencies', 'devDependencies', 'optionalDependencies']

export default function getSaveType (opts: PnpmOptions): DependenciesType | undefined {
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  if (opts.save) return 'dependencies'
  return undefined
}
