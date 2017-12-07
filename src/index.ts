import makeFetchHappen = require('make-fetch-happen')

const CORGI_DOC = 'application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*'
const JSON_DOC = 'application/json'

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
  const fetch = makeFetchHappen.defaults({
    ca: defaultOpts.ca,
    cacheManager: null,
    cert: defaultOpts.cert,
    key: defaultOpts.key,
    localAddress: defaultOpts.localAddress,
    proxy: defaultOpts.proxy,
    retry: defaultOpts.retry,
    strictSSL: defaultOpts.strictSSL,
  })

  return (url: string, opts?: {auth?: Auth}) => {
    const fetchOpts = {
      headers: getHeaders({auth: opts && opts.auth, userAgent: defaultOpts.userAgent}),
    }
    return fetch(url, fetchOpts)
  }
}

function getHeaders (opts: {auth?: Auth, userAgent?: string}) {
  const headers = {
    accept: CORGI_DOC,
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
