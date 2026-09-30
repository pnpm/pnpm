import type { Agent } from 'node:http'

import { pickSettingByUrl } from '@pnpm/network.config'
import { getProxyAgent, type ProxyAgentOptions } from '@pnpm/network.proxy-agent'
import HttpAgent from 'agentkeepalive'
import { LRUCache } from 'lru-cache'

const HttpsAgent = HttpAgent.HttpsAgent

const DEFAULT_MAX_SOCKETS = 50

const AGENT_CACHE = new LRUCache<string, Agent>({ max: 50 })

export type AgentOptions = ProxyAgentOptions & {
  noProxy?: boolean | string
}

export function getAgent (uri: string, opts: AgentOptions): Agent | undefined {
  if ((opts.httpProxy || opts.httpsProxy) && !checkNoProxy(uri, opts)) {
    const proxyAgent = getProxyAgent(uri, opts)
    if (proxyAgent) return proxyAgent
  }
  return getNonProxyAgent(uri, opts)
}

function getNonProxyAgent (uri: string, opts: AgentOptions): Agent | undefined {
  const isHttps = new URL(uri).protocol === 'https:'
  const { ca, cert, key: certKey } = {
    ...opts,
    ...pickSettingByUrl(opts.clientCertificates, uri),
  }

  // Node.js verifies certificates unless told otherwise, so an omitted
  // strictSsl must not share a cached agent with an explicit false.
  const strictSsl = opts.strictSsl ?? true
  const maxSockets = opts.maxSockets ?? DEFAULT_MAX_SOCKETS
  // If opts.timeout is zero, set the agentTimeout to zero as well. A timeout
  // of zero disables the timeout behavior (OS limits still apply). Else, if
  // opts.timeout is a non-zero value, set it to timeout + 1, to ensure that
  // the node-fetch-npm timeout will always fire first, giving us more
  // consistent errors.
  const agentTimeout =
    typeof opts.timeout !== 'number' || opts.timeout === 0
      ? 0
      : opts.timeout + 1

  const key = buildAgentCacheKey({
    agentTimeout,
    ca,
    cert,
    certKey,
    isHttps,
    localAddress: opts.localAddress,
    maxSockets,
    strictSsl,
  })

  const cachedAgent = AGENT_CACHE.get(key)
  if (cachedAgent) {
    return cachedAgent
  }

  const agent = createAgent({
    agentTimeout,
    ca,
    cert,
    certKey,
    isHttps,
    localAddress: opts.localAddress,
    maxSockets,
    strictSsl,
  })
  AGENT_CACHE.set(key, agent)
  return agent
}

interface AgentParams {
  agentTimeout: number
  ca?: unknown
  cert?: unknown
  certKey?: unknown
  isHttps: boolean
  localAddress?: string
  maxSockets: number
  strictSsl: boolean
}

function buildAgentCacheKey (params: AgentParams): string {
  const { agentTimeout, ca, cert, certKey, isHttps, localAddress, maxSockets, strictSsl } = params
  return [
    `https:${isHttps.toString()}`,
    `local-address:${localAddress ?? '>no-local-address<'}`,
    `max-sockets:${maxSockets}`,
    `timeout:${agentTimeout}`,
    `strict-ssl:${isHttps ? strictSsl.toString() : '>no-strict-ssl<'}`,
    `ca:${(isHttps && ca?.toString()) || '>no-ca<'}`,
    `cert:${(isHttps && cert?.toString()) || '>no-cert<'}`,
    `key:${(isHttps && certKey?.toString()) || '>no-key<'}`,
  ].join(':')
}

function createAgent (params: AgentParams): Agent {
  const { agentTimeout, ca, cert, certKey, isHttps, localAddress, maxSockets, strictSsl } = params
  // NOTE: localAddress is passed to the agent here even though it is an
  // undocumented option of the agent's constructor.
  //
  // This works because all options of the agent are merged with
  // all options of the request:
  // https://github.com/nodejs/node/blob/350a95b89faab526de852d417bbb8a3ac823c325/lib/_http_agent.js#L254
  return isHttps
    ? new HttpsAgent({
      ca,
      cert,
      key: certKey,
      localAddress,
      maxSockets,
      rejectUnauthorized: strictSsl,
      timeout: agentTimeout,
    } as any) // eslint-disable-line @typescript-eslint/no-explicit-any -- localAddress is an undocumented agent option missing from the typings
    : new HttpAgent({
      localAddress,
      maxSockets,
      timeout: agentTimeout,
    } as any) // eslint-disable-line @typescript-eslint/no-explicit-any -- localAddress is an undocumented agent option missing from the typings
}

function checkNoProxy (uri: string, opts: { noProxy?: boolean | string }): boolean | string | undefined {
  if (typeof opts.noProxy !== 'string') {
    return opts.noProxy
  }
  const hostLabels = new URL(uri).hostname
    .split('.')
    .filter(Boolean)
    .reverse()
  const noproxyEntries = opts.noProxy.split(',').map((entry) => entry.trim())
  return noproxyEntries.some((entry) => matchesNoProxyEntry(hostLabels, entry))
}

function matchesNoProxyEntry (hostLabels: string[], entry: string): boolean {
  const entryParts = entry
    .split('.')
    .filter(Boolean)
    .reverse()
  if (entryParts.length === 0) {
    return false
  }
  return entryParts.every((part, index) => hostLabels[index] === part)
}
