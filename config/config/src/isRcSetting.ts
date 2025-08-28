import npmTypes from '@pnpm/npm-conf/lib/types'
import { types } from './types.js'

export const isRcSetting = (kebabKey: string, extraTypes: Record<string, unknown> = {}): boolean =>
  kebabKey.startsWith('@') || kebabKey.startsWith('//') || kebabKey in npmTypes || kebabKey in types || kebabKey in extraTypes
