import {
  Dependencies,
  PackageBin,
  PackageManifest,
  PnpmOptions,
  StrictPnpmOptions,
} from '@pnpm/types'
import {LogBase} from '@pnpm/logger'
import {StoreController} from 'package-store'

export type WantedDependency = {
  alias?: string,
  pref: string, // package reference
  dev: boolean,
  optional: boolean,
  raw: string, // might be not needed
}

export type SupiOptions = PnpmOptions & {
  storeController?: StoreController
}

export type StrictSupiOptions = StrictPnpmOptions & {
  storeController?: StoreController
  pending?: boolean
}
