import { type DependenciesField } from './misc.js'
import { type BaseManifest, type DependencyManifest } from './package.js'

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

export interface FinderContext {
  name: string
  version: string
  readManifest: () => DependencyManifest
}

export type Finder = (ctx: FinderContext) => boolean | string
