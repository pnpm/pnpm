import { DependenciesField, PnpmOptions } from '@pnpm/types'

export default function getSaveType (opts: PnpmOptions): DependenciesField | undefined {
  if (opts.saveDev) return 'devDependencies'
  if (opts.saveOptional) return 'optionalDependencies'
  if (opts.saveProd) return 'dependencies'
  return undefined
}
