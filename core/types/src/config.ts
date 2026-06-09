export type PackageVersionPolicy = (pkgName: string) => boolean | string[]

export interface AllowBuildContext {
  depPath?: string
  trustPackageIdentity?: boolean
}

export type AllowBuild = (pkgName: string, pkgVersion: string, context?: AllowBuildContext) => boolean | undefined

export type TrustPolicy = 'no-downgrade' | 'off'
