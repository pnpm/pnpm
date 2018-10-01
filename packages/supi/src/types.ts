import { LogBase } from '@pnpm/logger'
import {
  PnpmOptions,
  StrictPnpmOptions,
} from '@pnpm/types'
import { StoreController } from 'package-store'

export type SupiOptions = PnpmOptions & {
  storeController: StoreController,
}

export type StrictSupiOptions = StrictPnpmOptions & {
  storeController: StoreController
  pending?: boolean,
}

export type ReporterFunction = (logObj: LogBase) => void
