import { URL } from 'url'
import HttpAgent from 'agentkeepalive'
import createHttpProxyAgent from 'http-proxy-agent'
import createHttpsProxyAgent from 'https-proxy-agent'
import LRU from 'lru-cache'
import SocksProxyAgent from 'socks-proxy-agent'

const HttpsAgent = HttpAgent.HttpsAgent

const DEFAULT_MAX_SOCKETS = 50

const AGENT_CACHE = new LRU({ max: 50 })

export interface AgentOptions {
  ca?: string
  cert?: string
  httpProxy?: string
  httpsProxy?: string
  key?: string
  localAddress?: string
  maxSockets?: number
  noProxy?: boolean | string
  strictSsl?: boolean
  timeout?: number
}

export default function getAgent (uri: string, opts: AgentOptions) {
  const parsedUri = new URL(uri)
  const isHttps = parsedUri.protocol === 'https:'
  const pxuri = getProxyUri(uri, opts)

  /* eslint-disable @typescript-eslint/prefer-nullish-coalescing */
  const key = [
    `https:${isHttps.toString()}`,
    pxuri
      ? `proxy:${pxuri.protocol}//${pxuri.username}:${pxuri.password}@${pxuri.host}:${pxuri.port}`
      : '>no-proxy<',
    `local-address:${opts.localAddress ?? '>no-local-address<'}`,
    `strict-ssl:${isHttps ? Boolean(opts.strictSsl).toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && opts.ca) || '>no-ca<'}`,
    `cert:${(isHttps && opts.cert) || '>no-cert<'}`,
    `key:${(isHttps && opts.key) || '>no-key<'}`,
  ].join(':')
  /* eslint-enable @typescript-eslint/prefer-nullish-coalescing */

  if (AGENT_CACHE.peek(key)) {
    return AGENT_CACHE.get(key)
  }

  if (pxuri) {
    const proxy = getProxy(pxuri, opts, isHttps)
    AGENT_CACHE.set(key, proxy)
    return proxy
  }

  // If opts.timeout is zero, set the agentTimeout to zero as well. A timeout
  // of zero disables the timeout behavior (OS limits still apply). Else, if
  // opts.timeout is a non-zero value, set it to timeout + 1, to ensure that
  // the node-fetch-npm timeout will always fire first, giving us more
  // consistent errors.
  const agentTimeout = typeof opts.timeout !== 'number' || opts.timeout === 0 ? 0 : opts.timeout + 1

  // NOTE: localAddress is passed to the agent here even though it is an
  // undocumented option of the agent's constructor.
  //
  // This works because all options of the agent are merged with
  // all options of the request:
  // https://github.com/nodejs/node/blob/350a95b89faab526de852d417bbb8a3ac823c325/lib/_http_agent.js#L254
  const agent = isHttps
    ? new HttpsAgent({
      ca: opts.ca,
      cert: opts.cert,
      key: opts.key,
      localAddress: opts.localAddress,
      maxSockets: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
      rejectUnauthorized: opts.strictSsl,
      timeout: agentTimeout,
    } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
    : new HttpAgent({
      localAddress: opts.localAddress,
      maxSockets: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
      timeout: agentTimeout,
    } as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  AGENT_CACHE.set(key, agent)
  return agent
}

function checkNoProxy (uri: string, opts: { noProxy?: boolean | string }) {
  const host = new URL(uri).hostname.split('.').filter(x => x).reverse()
  if (typeof opts.noProxy === 'string') {
    const noproxyArr = opts.noProxy.split(/\s*,\s*/g)
    return noproxyArr.some(no => {
      const noParts = no.split('.').filter(x => x).reverse()
      if (noParts.length === 0) {
        return false
      }
      for (let i = 0; i < noParts.length; i++) {
        if (host[i] !== noParts[i]) {
          return false
        }
      }
      return true
    })
  }
  return opts.noProxy
}

function getProxyUri (
  uri: string,
  opts: {
    httpProxy?: string
    httpsProxy?: string
    noProxy?: boolean | string
  }
) {
  const { protocol } = new URL(uri)

  let proxy: string | undefined
  switch (protocol) {
  case 'http:': {
    proxy = opts.httpProxy
    break
  }
  case 'https:': {
    proxy = opts.httpsProxy
    break
  }
  }

  if (!proxy) {
    return null
  }

  if (!proxy.includes('://')) {
    proxy = `${protocol}//${proxy}`
  }

  const parsedProxy = (typeof proxy === 'string') ? new URL(proxy) : proxy

  return !checkNoProxy(uri, opts) && parsedProxy
}

function getProxy (
  proxyUrl: URL,
  opts: {
    ca?: string
    cert?: string
    key?: string
    timeout?: number
    localAddress?: string
    maxSockets?: number
    strictSsl?: boolean
  },
  isHttps: boolean
) {
  const popts = {
    auth: getAuth(proxyUrl),
    ca: opts.ca,
    cert: opts.cert,
    host: proxyUrl.hostname,
    key: opts.key,
    localAddress: opts.localAddress,
    maxSockets: opts.maxSockets ?? DEFAULT_MAX_SOCKETS,
    path: proxyUrl.pathname,
    port: proxyUrl.port,
    protocol: proxyUrl.protocol,
    rejectUnauthorized: opts.strictSsl,
    timeout: typeof opts.timeout !== 'number' || opts.timeout === 0 ? 0 : opts.timeout + 1,
  }

  if (proxyUrl.protocol === 'http:' || proxyUrl.protocol === 'https:') {
    if (!isHttps) {
      return createHttpProxyAgent(popts)
    } else {
      return createHttpsProxyAgent(popts)
    }
  }
  if (proxyUrl.protocol?.startsWith('socks')) {
    return new SocksProxyAgent(popts)
  }
}

function getAuth (user: { username?: string, password?: string }) {
  if (!user.username) {
    return undefined
  }
  let auth = user.username
  if (user.password) {
    auth += `:${user.password}`
  }
  return decodeURIComponent(auth)
}
