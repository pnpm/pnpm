import { DependenciesField } from './misc'
import { PackageManifest, ProjectManifest } from './package'

export type LogBase = {
  level: 'debug' | 'error'
} | {
  level: 'info' | 'warn'
  prefix: string
  message: string
}

export type IncludedDependencies = {
  [dependenciesField in DependenciesField]: boolean
}

export interface ReadPackageHook {
  (pkg: PackageManifest): PackageManifest
  (pkg: ProjectManifest): ProjectManifest
}
