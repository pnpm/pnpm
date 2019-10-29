import logger from '@pnpm/logger'

export const scopeLogger = logger<{ selected: number, total?: number, workspacePrefix: string | null }>('scope')
