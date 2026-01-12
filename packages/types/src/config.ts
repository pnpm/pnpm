export type PackageVersionPolicy = (pkgName: string) => boolean | string[]

export type AllowBuild = (pkgName: string, pkgVersion: string) => boolean | undefined

export type TrustPolicy = 'no-downgrade' | 'off'
