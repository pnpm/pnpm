export default class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string
  public attempts?: number
  public pkgsStack?: Array<{ id: string, name: string, version: string }>
  constructor (
    code: string,
    message: string,
    opts?: {
      attempts?: number
      hint?: string
    }
  ) {
    super(message)
    this.code = `ERR_PNPM_${code}`
    this.hint = opts?.hint
    this.attempts = opts?.attempts
  }
}

export interface FetchErrorResponse { status: number, statusText: string }

export interface FetchErrorRequest { url: string, authHeaderValue?: string }

export class FetchError extends PnpmError {
  public readonly response: FetchErrorResponse
  public readonly request: FetchErrorRequest

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    hint?: string
  ) {
    const message = `GET ${request.url}: ${response.statusText} - ${response.status}`
    const authHeaderValue = request.authHeaderValue
      ? hideAuthInformation(request.authHeaderValue) : undefined
    // NOTE: For security reasons, some registries respond with 404 on authentication errors as well.
    // So we print authorization info on 404 errors as well.
    if (response.status === 401 || response.status === 403 || response.status === 404) {
      hint = hint ? `${hint}\n\n` : ''
      if (authHeaderValue) {
        hint += `An authorization header was used: ${authHeaderValue}`
      } else {
        hint += 'No authorization header was set for the request.'
      }
    }
    super(`FETCH_${response.status}`, message, { hint })
    this.request = request
    this.response = response
  }
}

function hideAuthInformation (authHeaderValue: string) {
  const [authType, token] = authHeaderValue.split(' ')
  return `${authType} ${token.substring(0, 4)}[hidden]`
}
