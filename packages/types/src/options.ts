import { type DependenciesField } from './misc'
import { type PackageManifest, type ProjectManifest } from './package'

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
  (pkg: PackageManifest, dir?: string): PackageManifest | Promise<PackageManifest>
  (pkg: ProjectManifest, dir?: string): ProjectManifest | Promise<ProjectManifest>
}
