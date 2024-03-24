import { logger, type Logger } from '@pnpm/logger'

export const lockfileLogger: Logger<{ message: string; prefix: string; }> = logger<{ message: string; prefix: string; }>('lockfile')
