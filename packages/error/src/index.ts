export default class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string
  public pkgsStack?: Array<{ id: string, name: string, version: string }>
  constructor (code: string, message: string, opts?: { hint?: string }) {
    super(message)
    this.code = `ERR_PNPM_${code}`
    this.hint = opts?.hint
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
    if (response.status === 401 || response.status === 403) {
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
