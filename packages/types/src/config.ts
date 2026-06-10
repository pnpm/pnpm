import { type DepPath } from './misc.js'

export type PackageVersionPolicy = (pkgName: string) => boolean | string[]

export type AllowBuild = (depPath: DepPath) => boolean

export type TrustPolicy = 'no-downgrade' | 'off'
