import fetch from '@pnpm/fetch'
import npmRegistryAgent from '@pnpm/npm-registry-agent'

const USER_AGENT = 'pnpm' // or maybe make it `${pkg.name}/${pkg.version} (+https://npm.im/${pkg.name})`

const CORGI_DOC = 'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'
const MAX_FOLLOWED_REDIRECTS = 20

export type Auth = (
  {token: string} |
  {username: string, password: string} |
  {_auth: string}
) & {
  token?: string,
  username?: string,
  password?: string,
  _auth?: string,
}

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
  },
) {
  return async (url: string, opts?: {auth?: Auth}) => {
    const agent = npmRegistryAgent(url, {
      ...defaultOpts,
      ...opts,
    } as any) // tslint:disable-line
    const headers = {
      'connection': agent ? 'keep-alive' : 'close',
      'user-agent': USER_AGENT,
      ...getHeaders({
        auth: opts && opts.auth,
        fullMetadata: defaultOpts.fullMetadata,
        userAgent: defaultOpts.userAgent,
      }),
    }

    let redirects = 0
    while (true) {
      let response = await fetch(url, {
        agent,
        // if verifying integrity, node-fetch must not decompress
        compress: false,
        headers,
        redirect: 'manual',
        retry: defaultOpts.retry,
      })
      if (!fetch.isRedirect(response.status) || redirects >= MAX_FOLLOWED_REDIRECTS) {
        return response
      }

      // This is a workaround to remove authorization headers on redirect.
      // It is needed until node-fetch fixes this
      // or supports a way to do it via an option.
      // node-fetch issue: https://github.com/bitinn/node-fetch/issues/274
      // Related pnpm issue: https://github.com/pnpm/pnpm/issues/1815
      redirects++
      url = response.headers.get('location')
      delete headers['authorization']
    }
  }
}

function getHeaders (
  opts: {
    auth?: Auth,
    fullMetadata?: boolean,
    userAgent?: string,
  },
) {
  const headers = {
    accept: opts.fullMetadata === true ? JSON_DOC : CORGI_DOC,
  }
  if (opts.auth) {
    const authorization = authObjectToHeaderValue(opts.auth)
    if (authorization) {
      headers['authorization'] = authorization // tslint:disable-line
    }
  }
  if (opts.userAgent) {
    headers['user-agent'] = opts.userAgent
  }
  return headers
}

function authObjectToHeaderValue (auth: Auth) {
  if (auth.token) {
    return `Bearer ${auth.token}`
  }
  if (auth.username && auth.password) {
    const encoded = Buffer.from(
      `${auth.username}:${auth.password}`, 'utf8',
    ).toString('base64')
    return `Basic ${encoded}`
  }
  if (auth._auth) {
    return `Basic ${auth._auth}`
  }
  return undefined
}
