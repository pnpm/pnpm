import { isError } from '@pnpm/error'
import { logger } from '@pnpm/logger'

import type { HoistedModulesDirs, HoistType } from './types.js'

export const hoistLogger = logger('hoist')

export function selectHoistedModulesDir (hoistType: HoistType, dirs: HoistedModulesDirs): string {
  return hoistType === 'public'
    ? dirs.publicHoistedModulesDir
    : dirs.privateHoistedModulesDir
}

export function hasErrorCode (error: unknown, code: string): boolean {
  return isError(error) && 'code' in error && error.code === code
}
