import {
  Dependencies,
  PackageBin,
  PackageManifest,
} from '@pnpm/types'
import {LogBase} from '@pnpm/logger'

export type WantedDependency = {
  alias?: string,
  pref: string, // package reference
  dev: boolean,
  optional: boolean,
  raw: string, // might be not needed
}
