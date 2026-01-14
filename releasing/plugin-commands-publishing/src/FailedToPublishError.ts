import { PnpmError } from '@pnpm/error'
import { type PackResult } from './pack.js'

interface PublishErrorProperties {
  readonly pack: PackResult
  readonly status: number
  readonly statusText: string
  readonly text: string
}

export class FailedToPublishError extends PnpmError implements PublishErrorProperties {
  readonly pack: PackResult
  readonly status: number
  readonly statusText: string
  readonly text: string

  constructor (opts: PublishErrorProperties) {
    const { pack, status, statusText, text } = opts
    const { name, version } = pack.publishedManifest

    const statusDisplay = statusText ? `${status} ${statusText}` : status

    const trimmedText = text.trim()
    let message = `Failed to publish package ${name}@${version} (status ${statusDisplay})`
    if (trimmedText.includes('\n')) {
      message += '\nDetails:\n'
      for (const line of text.trimEnd().split('\n')) {
        message += `    ${line}\n`
      }
    } else if (trimmedText) {
      message += `: ${trimmedText}`
    }

    super('FAILED_TO_PUBLISH', message)

    this.pack = pack
    this.status = status
    this.statusText = statusText
    this.text = text
  }

  static async createFailedToPublishError (pack: PackResult, fetchResponse: FetchResponse): Promise<FailedToPublishError> {
    const { status, statusText } = fetchResponse

    let text: string
    try {
      text = await fetchResponse.text()
    } catch {
      text = ''
    }

    return new FailedToPublishError({ pack, status, statusText, text })
  }
}

interface FetchResponse {
  readonly status: number
  readonly statusText: string
  readonly text: (this: FetchResponse) => string | Promise<string>
}
