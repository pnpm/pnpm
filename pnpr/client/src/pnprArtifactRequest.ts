import http from 'node:http'
import https from 'node:https'
import { URL } from 'node:url'

import { REQUEST_TIMEOUT } from './sharedArtifactTypes.js'

export interface RequestOptions {
  registryUrl: string
  path: string
  method: 'GET' | 'POST' | 'PUT'
  authorization?: string
  body?: Buffer
  maxResponseSize: number
}

export interface BufferedResponse {
  statusCode: number
  body: Buffer
}

interface ResponseCollector {
  maxResponseSize: number
  resolve: (response: BufferedResponse) => void
  reject: (err: Error) => void
}

export async function request (opts: RequestOptions): Promise<BufferedResponse> {
  const url = artifactEndpointUrl(opts)
  const requestFn = url.protocol === 'https:' ? https.request : http.request
  const headers = requestHeaders(opts)

  return new Promise((resolve, reject) => {
    const req = requestFn(url, {
      method: opts.method,
      timeout: REQUEST_TIMEOUT,
      headers,
    }, (res) => {
      collectResponse(res, { maxResponseSize: opts.maxResponseSize, resolve, reject })
    })
    req.on('timeout', () => req.destroy(new Error(`pnpr server request timed out after ${REQUEST_TIMEOUT / 1000}s`)))
    req.on('error', reject)
    req.end(opts.body)
  })
}

function artifactEndpointUrl (opts: Pick<RequestOptions, 'registryUrl' | 'path'>): URL {
  const base = opts.registryUrl.endsWith('/') ? opts.registryUrl : `${opts.registryUrl}/`
  const url = new URL(opts.path, base)
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error(`Unsupported pnpr registry protocol ${JSON.stringify(url.protocol)}`)
  }
  return url
}

function requestHeaders (opts: Pick<RequestOptions, 'authorization' | 'body'>): http.OutgoingHttpHeaders {
  const headers: http.OutgoingHttpHeaders = {}
  if (opts.body != null) {
    headers['Content-Type'] = 'application/json'
    headers['Content-Length'] = opts.body.byteLength
  }
  if (opts.authorization != null) headers.Authorization = opts.authorization
  return headers
}

function collectResponse (res: http.IncomingMessage, collector: ResponseCollector): void {
  const { maxResponseSize } = collector
  const declaredLength = Number(res.headers['content-length'])
  if (Number.isFinite(declaredLength) && declaredLength > maxResponseSize) {
    res.destroy(new Error(`pnpr response exceeds ${maxResponseSize} bytes`))
    return
  }
  const chunks: Buffer[] = []
  let received = 0
  res.on('data', (chunk: Buffer) => {
    received += chunk.byteLength
    if (received > maxResponseSize) {
      res.destroy(new Error(`pnpr response exceeds ${maxResponseSize} bytes`))
      return
    }
    chunks.push(chunk)
  })
  res.on('end', () => collector.resolve({ statusCode: res.statusCode ?? 0, body: Buffer.concat(chunks) }))
  res.on('error', collector.reject)
}

export function assertSuccess (response: BufferedResponse, endpoint: string): void {
  if (response.statusCode < 200 || response.statusCode >= 300) {
    const body = response.body.toString('utf8').slice(0, 1_024)
    throw new Error(`pnpr server ${endpoint} responded with ${response.statusCode}: ${body}`)
  }
}
