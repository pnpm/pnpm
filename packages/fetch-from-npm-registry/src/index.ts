import fetch, { isRedirect, Response } from '@pnpm/fetch'
import npmRegistryAgent from '@pnpm/npm-registry-agent'
import { URL } from 'url'

const USER_AGENT = 'pnpm' // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const CORGI_DOC = 'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'
const MAX_FOLLOWED_REDIRECTS = 20

export type FetchFromRegistry = (url: string, opts?: { authHeaderValue?: string }) => Promise<Response>

export default function (
  defaultOpts: {
    fullMetadata?: boolean,
    // proxy
    proxy?: string,
    localAddress?: string,
    // ssl
    ca?: string,
    cert?: string,
    key?: string,
    strictSSL?: boolean,
    // retry
    retry?: {
      retries?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
    userAgent?: string,
  }
): FetchFromRegistry {
  return async (url, opts): Promise<Response> => {
    const headers = {
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts?.authHeaderValue,
        fullMetadata: defaultOpts.fullMetadata,
        userAgent: defaultOpts.userAgent,
      }),
    }

    let redirects = 0
    let urlObject = new URL(url)
    const originalHost = urlObject.host
    while (true) {
      const agent = npmRegistryAgent(urlObject.href, {
        ...defaultOpts,
        ...opts,
      } as any) // tslint:disable-line
      headers['connection'] = agent ? 'keep-alive' : 'close'

      // We should pass a URL object to node-fetch till this is not resolved:
      // https://github.com/bitinn/node-fetch/issues/245
      let response = await fetch(urlObject, {
        agent,
        // if verifying integrity, node-fetch must not decompress
        compress: false,
        headers,
        redirect: 'manual',
        retry: defaultOpts.retry,
      })
      if (!isRedirect(response.status) || redirects >= MAX_FOLLOWED_REDIRECTS) {
        return response
      }

      // This is a workaround to remove authorization headers on redirect.
      // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
      redirects++
      urlObject = new URL(response.headers.get('location')!)
      if (!headers['authorization'] || originalHost === urlObject.host) continue
      delete headers['authorization']
    }
  }
}

function getHeaders (
  opts: {
    auth?: string,
    fullMetadata?: boolean,
    userAgent?: string,
  }
) {
  const headers = {
    accept: opts.fullMetadata === true ? JSON_DOC : CORGI_DOC,
  }
  if (opts.auth) {
    headers['authorization'] = opts.auth // tslint:disable-line
  }
  if (opts.userAgent) {
    headers['user-agent'] = opts.userAgent
  }
  return headers
}
