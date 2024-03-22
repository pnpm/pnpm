import '@total-typescript/ts-reset'
import { WANTED_LOCKFILE } from '@pnpm/constants'

export class PnpmError extends Error {
  public readonly code: string
  public readonly hint?: string | undefined
  public attempts?: number | undefined
  public prefix?: string | undefined
  public pkgsStack?: Array<{ id: string; name: string; version: string }> | undefined
  public failures?: Array<{ message: string; prefix: string }> | undefined

  public passes?: number | undefined
  constructor(
    code: string,
    message: string,
    opts?: {
      attempts?: number | undefined
      hint?: string | undefined
    } | undefined
  ) {
    super(message)

    this.code = code.startsWith('ERR_PNPM_') ? code : `ERR_PNPM_${code}`

    this.hint = opts?.hint

    this.attempts = opts?.attempts
  }
}

export type FetchErrorResponse = {
  status: number
  statusText: string
}

export type FetchErrorRequest = {
  url: string
  authHeaderValue?: string | undefined
}

export class FetchError extends PnpmError {
  public readonly response: FetchErrorResponse
  public readonly request: FetchErrorRequest

  constructor(
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    hint?: string | undefined
  ) {
    const message = `GET ${request.url}: ${response.statusText} - ${response.status}`

    const authHeaderValue = request.authHeaderValue
      ? hideAuthInformation(request.authHeaderValue)
      : undefined

    // NOTE: For security reasons, some registries respond with 404 on authentication errors as well.
    // So we print authorization info on 404 errors as well.
    if (
      response.status === 401 ||
      response.status === 403 ||
      response.status === 404
    ) {
      hint = hint ? `${hint}\n\n` : ''

      hint += authHeaderValue ? `An authorization header was used: ${authHeaderValue}` : 'No authorization header was set for the request.';
    }

    super(`FETCH_${response.status}`, message, { hint })
    this.request = request
    this.response = response
  }
}

function hideAuthInformation(authHeaderValue: string): string {
  const [authType, token] = authHeaderValue.split(' ')

  return `${authType} ${token?.substring(0, 4) ?? ''}[hidden]`
}

export class LockfileMissingDependencyError extends PnpmError {
  constructor(depPath: string) {
    const message = `Broken lockfile: no entry for '${depPath}' in ${WANTED_LOCKFILE}`

    super('LOCKFILE_MISSING_DEPENDENCY', message, {
      hint:
        'This issue is probably caused by a badly resolved merge conflict.\n' +
        "To fix the lockfile, run 'pnpm install --no-frozen-lockfile'.",
    })
  }
}
