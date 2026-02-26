import { PnpmError } from '@pnpm/error'
import { type PackResult } from './pack.js'

interface PublishErrorProperties<Pack> {
  readonly pack: Pack
  readonly status: number
  readonly statusText: string
  readonly text: string
}

export class FailedToPublishError<Pack extends Pick<PackResult, 'publishedManifest'>> extends PnpmError implements PublishErrorProperties<Pack> {
  readonly pack: Pack
  readonly status: number
  readonly statusText: string
  readonly text: string

  constructor (opts: PublishErrorProperties<Pack>) {
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
}

export async function createFailedToPublishError<Pack extends Pick<PackResult, 'publishedManifest'>> (
  pack: Pack,
  fetchResponse: FetchResponse
): Promise<FailedToPublishError<Pack>> {
  const { status, statusText } = fetchResponse

  let text: string
  try {
    text = await fetchResponse.text()
  } catch {
    text = ''
  }

  return new FailedToPublishError({ pack, status, statusText, text })
}

interface FetchResponse {
  readonly status: number
  readonly statusText: string
  readonly text: (this: FetchResponse) => string | Promise<string>
}
