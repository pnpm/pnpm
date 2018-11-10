import { LogBase } from '@pnpm/logger'
import { StoreController } from '@pnpm/store-controller-types'
import {
  PnpmOptions,
  StrictPnpmOptions,
} from '@pnpm/types'

export type SupiOptions = PnpmOptions & {
  storeController: StoreController,
}

export type StrictSupiOptions = StrictPnpmOptions & {
  storeController: StoreController
  pending?: boolean,
}

export type ReporterFunction = (logObj: LogBase) => void
