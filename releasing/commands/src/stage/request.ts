import { globalWarn } from '@pnpm/logger'
import { SyntheticOtpError, type WebAuthFetchOptions, withOtpHandling } from '@pnpm/network.web-auth'

import { createPublishContext } from '../publish/publishPackedPkg.js'
import type { StageContext } from './context.js'
import { StageRegistryError } from './errors.js'
import type { StageOptions } from './types.js'

export interface StageRequestInit {
  body?: string
  headers?: Record<string, string>
  method: 'DELETE' | 'GET' | 'POST'
}

interface StageRequestParams {
  url: string
  action: string
  init?: StageRequestInit
  otp?: string
}

export async function stageJsonRequest<T> (
  context: StageContext,
  params: { url: string, action: string }
): Promise<T> {
  const response = await stageRequest(context, {
    url: params.url,
    action: params.action,
    init: { method: 'GET' },
  })
  return await response.json() as T
}

/**
 * Wraps {@link stageRequest} with OTP / web-auth handling. The first attempt
 * carries any user-configured `--otp`; if the registry responds with an OTP
 * challenge, `withOtpHandling` drives the browser-based authentication flow
 * and retries the operation with the resulting token.
 */
export async function stageRequestWithOtp (
  context: StageContext,
  params: { url: string, init: StageRequestInit, action: string }
): Promise<Response> {
  return withOtpHandling({
    context: createPublishContext(context.opts),
    fetchOptions: createWebAuthFetchOptions(context.opts),
    operation: async (otp) => stageRequest(context, {
      url: params.url,
      action: params.action,
      init: params.init,
      otp: otp ?? getConfiguredOtp(context.opts),
    }),
  })
}

export async function stageRequest (context: StageContext, params: StageRequestParams): Promise<Response> {
  const init = params.init ?? { method: 'GET' }
  const response = await context.fetchFromRegistry(params.url, {
    authHeaderValue: context.authHeaderValue,
    body: init.body,
    fullMetadata: true,
    headers: {
      'npm-auth-type': 'web',
      'npm-command': 'stage',
      ...init.headers,
      ...(params.otp != null ? { 'npm-otp': params.otp } : {}),
    },
    method: init.method,
    timeout: context.opts.fetchTimeout,
  })
  if (!response.ok) {
    await throwOnErrorResponse(response, params.action)
  }
  return response
}

async function throwOnErrorResponse (response: Response, action: string): Promise<never> {
  let text = ''
  try {
    text = await response.text()
  } catch {}
  let parsed: unknown
  try {
    parsed = text ? JSON.parse(text) : undefined
  } catch {}

  if (response.status === 401 && isOtpChallenge(response, parsed)) {
    throw SyntheticOtpError.fromUnknownBody(globalWarn, parsed)
  }
  throw new StageRegistryError({
    action,
    status: response.status,
    statusText: response.statusText,
    text,
  })
}

/**
 * Identify a 401 response as an OTP / web-auth challenge.
 *
 * Two signals are accepted because the npm registry uses one or both in
 * practice: the legacy `www-authenticate: otp` header for classic TOTP,
 * and a JSON body containing `authUrl` + `doneUrl` for the browser-based
 * web-auth flow.
 */
function isOtpChallenge (response: Response, body: unknown): boolean {
  if (hasWebAuthUrls(body)) return true
  const wwwAuthenticate = response.headers.get('www-authenticate')?.toLowerCase()
  return wwwAuthenticate?.includes('otp') === true
}

function hasWebAuthUrls (body: unknown): boolean {
  if (body == null || typeof body !== 'object') return false
  const record = body as Record<string, unknown>
  return typeof record.authUrl === 'string' && typeof record.doneUrl === 'string'
}

function getConfiguredOtp (opts: StageOptions): string | undefined {
  if (typeof opts.otp === 'string') return opts.otp
  const cliOtp = opts.cliOptions?.otp
  return typeof cliOtp === 'string' ? cliOtp : undefined
}

function createWebAuthFetchOptions (opts: StageOptions): WebAuthFetchOptions {
  return {
    method: 'GET',
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  }
}
