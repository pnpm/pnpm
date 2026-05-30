export interface AddUserResponse {
  ok: boolean
  status: number
  text: () => Promise<string>
  json: () => Promise<unknown>
  headers: { get: (name: string) => string | null }
}

export interface AddUserFetch {
  (url: string, init: {
    method: 'PUT'
    headers: Record<string, string>
    body: string
  }): Promise<AddUserResponse>
}

export interface AddUserOptions {
  username: string
  password: string
  email: string
  registryUrl: string
  fetch: AddUserFetch
  otp?: string
}

export interface AddUserResult {
  token: string
}

export class AddUserHttpError extends Error {
  readonly status: number
  readonly responseText: string
  readonly responseJson: unknown | undefined
  readonly responseHeaders: { get: (name: string) => string | null }
  constructor (status: number, responseText: string, responseHeaders: { get: (name: string) => string | null }) {
    super(`addUser failed (HTTP ${status}): ${responseText}`)
    this.name = 'AddUserHttpError'
    this.status = status
    this.responseText = responseText
    this.responseHeaders = responseHeaders
    try {
      this.responseJson = JSON.parse(responseText)
    } catch {
      this.responseJson = undefined
    }
  }
}

export class AddUserNoTokenError extends Error {
  constructor () {
    super('The registry returned a successful response but no token')
    this.name = 'AddUserNoTokenError'
  }
}

export async function addUser (opts: AddUserOptions): Promise<AddUserResult> {
  const url = new URL(`-/user/org.couchdb.user:${encodeURIComponent(opts.username)}`, opts.registryUrl).href
  const response = await opts.fetch(url, {
    method: 'PUT',
    headers: {
      'content-type': 'application/json',
      accept: 'application/json',
      'npm-auth-type': 'web',
      ...(opts.otp != null ? { 'npm-otp': opts.otp } : {}),
    },
    body: JSON.stringify({
      _id: `org.couchdb.user:${opts.username}`,
      name: opts.username,
      password: opts.password,
      email: opts.email,
      type: 'user',
    }),
  })
  if (!response.ok) {
    const text = await response.text()
    throw new AddUserHttpError(response.status, text, response.headers)
  }
  const body = await response.json() as { token?: string } | null
  if (!body?.token) {
    throw new AddUserNoTokenError()
  }
  return { token: body.token }
}
