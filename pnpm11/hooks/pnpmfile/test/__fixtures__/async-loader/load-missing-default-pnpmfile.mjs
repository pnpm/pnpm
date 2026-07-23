import { requireHooks } from '@pnpm/hooks.pnpmfile'

await requireHooks(import.meta.dirname, { tryLoadDefaultPnpmfile: true })
