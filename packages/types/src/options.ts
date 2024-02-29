import { type DependenciesField } from './misc'
import { type BaseManifest } from './package'

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

export type ReadPackageHook = <Pkg extends BaseManifest> (pkg: Pkg, dir?: string) => Pkg | Promise<Pkg>
