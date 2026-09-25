import { PnpmError } from '@pnpm/error'

interface StageRegistryErrorProperties {
  readonly action: string
  readonly status: number
  readonly statusText: string
  readonly text: string
}

export class StageRegistryError extends PnpmError implements StageRegistryErrorProperties {
  readonly action: string
  readonly status: number
  readonly statusText: string
  readonly text: string

  constructor (opts: StageRegistryErrorProperties) {
    const statusDisplay = opts.statusText ? `${opts.status} ${opts.statusText}` : opts.status.toString()
    const trimmedText = opts.text.trim()
    super(
      'STAGE_REGISTRY_ERROR',
      `Failed to ${opts.action} (status ${statusDisplay})${trimmedText ? `: ${trimmedText}` : ''}`
    )
    this.action = opts.action
    this.status = opts.status
    this.statusText = opts.statusText
    this.text = opts.text
  }
}
