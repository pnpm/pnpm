export default class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string
  public pkgsStack?: Array<{ id: string, name: string, version: string }>
  constructor (code: string, message: string, opts?: { hint: string }) {
    super(message)
    this.code = `ERR_PNPM_${code}`
    this.hint = opts?.hint
  }
}

export type FetchErrorResponse = { status: number, statusText: string }

export type FetchErrorRequest = { url: string, authHeaderValue?: string }

export class FetchError extends PnpmError {
  public readonly response: FetchErrorResponse
  public readonly request: FetchErrorRequest

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    details?: string
  ) {
    let message = `GET ${request.url} ${response.statusText} (${response.status}).`
    if (details) {
      message += `\n${details}`
    }
    const authHeaderValue = request.authHeaderValue
      ? hideAuthInformation(request.authHeaderValue) : undefined
    if (response.status === 401 || response.status === 403) {
      message += `\n`
      if (authHeaderValue) {
        message += `An authorization header was used: ${authHeaderValue}`
      } else {
        message += `No authorization header was set for the request.`
      }
    }
    super(`FETCH_${response.status}`, message)
    this.request = request
    this.response = response
  }
}

function hideAuthInformation (authHeaderValue: string) {
  const [authType, token] = authHeaderValue.split(' ')
  return `${authType} ${token.substring(0, 4)}[hidden]`
}
