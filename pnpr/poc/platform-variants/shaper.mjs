// A CDN-like reverse proxy for benchmarks. It gzips responses for clients
// that accept it, caches successful GETs by URL and Accept header (a CDN edge
// cache), adds a round-trip delay to every request, and shares one bandwidth
// cap across all response bodies.
// usage: node shaper.mjs <listenPort> <targetPort>
// control: GET /__config?rtt=<ms>&mbit=<n>, GET /__flush, GET /__stats
// (returns and resets the counters).
import http from 'node:http'
import zlib from 'node:zlib'

const [listenPort, targetPort] = process.argv.slice(2)
let rtt = 0
let bytesPerMs = 0
const agent = new http.Agent({ keepAlive: true, maxSockets: 256 })
const cache = new Map()

let linkFreeAt = 0
function linkDelay (len) {
  if (bytesPerMs <= 0) return 0
  const now = performance.now()
  const start = Math.max(now, linkFreeAt)
  linkFreeAt = start + len / bytesPerMs
  return linkFreeAt - now
}

let stats = {}
function kind (url) {
  const path = url.split('?')[0]
  const last = path.split('/').pop() ?? ''
  if (path.includes('/-/') || path.includes('/variants/') || last.indexOf('@') > 0) return 'tarball'
  const parts = path.replace(/^\/npm\//, '').split('/').filter(Boolean)
  const nameParts = parts[0]?.startsWith('@') ? 2 : 1
  return parts.length > nameParts ? 'version-doc' : 'packument'
}
function count (url, bytes) {
  const s = stats[kind(url)] ??= { requests: 0, bytes: 0 }
  s.requests++
  s.bytes += bytes
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

async function send (res, status, headers, body) {
  res.writeHead(status, headers)
  const chunk = 64 * 1024
  for (let i = 0; i < body.length; i += chunk) {
    const part = body.subarray(i, i + chunk)
    const wait = linkDelay(part.length)
    if (wait > 1) await sleep(wait)
    res.write(part)
  }
  res.end()
}

function fetchUpstream (req) {
  return new Promise((resolve, reject) => {
    const upstream = http.request({
      host: '127.0.0.1', port: targetPort, path: req.url, method: req.method,
      headers: { ...req.headers, 'accept-encoding': 'identity' }, agent,
    }, async up => {
      const chunks = []
      for await (const c of up) chunks.push(c)
      resolve({ status: up.statusCode, headers: up.headers, body: Buffer.concat(chunks) })
    })
    upstream.on('error', reject)
    req.pipe(upstream)
  })
}

http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://x')
  if (url.pathname === '/__config') {
    rtt = Number(url.searchParams.get('rtt') ?? rtt)
    bytesPerMs = Number(url.searchParams.get('mbit') ?? 0) * 1e6 / 8 / 1000
    return res.end('ok')
  }
  if (url.pathname === '/__flush') { cache.clear(); return res.end('ok') }
  if (url.pathname === '/__stats') { res.end(JSON.stringify(stats)); stats = {}; return }
  await sleep(rtt)
  const gzip = /\bgzip\b/.test(req.headers['accept-encoding'] ?? '')
  const key = `${req.method} ${req.url} ${req.headers.accept ?? ''} ${gzip}`
  let entry = req.method === 'GET' ? cache.get(key) : undefined
  if (!entry) {
    let up
    try { up = await fetchUpstream(req) } catch (err) { res.writeHead(502); return res.end(String(err)) }
    const headers = { ...up.headers }
    delete headers['content-length']
    delete headers['transfer-encoding']
    let body = up.body
    if (gzip && up.status === 200 && !headers['content-encoding'] && body.length > 256) {
      body = zlib.gzipSync(body, { level: 6 })
      headers['content-encoding'] = 'gzip'
    }
    headers['content-length'] = String(body.length)
    entry = { status: up.status, headers, body }
    if (req.method === 'GET' && up.status === 200) cache.set(key, entry)
  }
  count(req.url, entry.body.length)
  await send(res, entry.status, entry.headers, entry.body)
}).listen(Number(listenPort), '127.0.0.1')
