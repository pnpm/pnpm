import type { DepPath } from './misc.js'

export type PackageVersionPolicy = (pkgName: string) => boolean | string[]

export interface AllowBuildContext {
  trustPackageIdentity?: boolean
}

export type AllowBuild = (depPath: DepPath, context?: AllowBuildContext) => boolean | undefined

export type TrustPolicy = 'no-downgrade' | 'off'
