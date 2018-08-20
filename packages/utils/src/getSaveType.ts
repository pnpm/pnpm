import {PnpmOptions} from '@pnpm/types'
export type DependenciesType = 'dependencies' | 'devDependencies' | 'optionalDependencies'

export const dependenciesTypes: DependenciesType[] = ['optionalDependencies', 'dependencies', 'devDependencies']

export default function getSaveType (opts: PnpmOptions): DependenciesType | undefined {
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  if (opts.saveProd) return 'dependencies'
  return undefined
}
