import { createStream } from './resources.mjs'

export async function request (resources, options, signal) {
  let url
  try {
    url = new URL(options.url)
  } catch {
    throw new TypeError('Invalid HTTP request URL')
  }
  if (url.username || url.password) throw new TypeError('HTTP URL credentials must be supplied as an Authorization header')
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error(`Unsupported HTTP protocol: ${url.protocol}`)
  }
  const controller = new AbortController()
  const timeoutMs = options.timeoutMs ?? 60000
  if (!Number.isInteger(timeoutMs) || timeoutMs < 1) throw new RangeError('Invalid HTTP timeout')
  if (options.totalTimeoutMs != null && (!Number.isInteger(options.totalTimeoutMs) || options.totalTimeoutMs < 1)) {
    throw new RangeError('Invalid HTTP total timeout')
  }
  const totalTimer = options.totalTimeoutMs == null ? null
    : setTimeout(() => controller.abort(new Error('HTTP request deadline timed out')), options.totalTimeoutMs)
  const timer = setTimeout(() => controller.abort(new Error('HTTP request timed out')), timeoutMs)
  const pending = resources.add({ kind: 'request', close: () => controller.abort() })
  try {
    const body = options.bodyHandle == null ? options.body : resources.get(options.bodyHandle, 'upload').readable
    const response = await fetch(url, {
      method: options.method ?? 'GET',
      headers: options.headers,
      body,
      ...(options.bodyHandle == null ? {} : { duplex: 'half' }),
      redirect: 'manual',
      signal: signal ? AbortSignal.any([controller.signal, signal]) : controller.signal,
    })
    const reader = response.body?.getReader()
    const stream = createStream(
      async () => {
        const chunk = await readWithTimeout(reader, controller, timeoutMs)
        if (chunk.done) clearTimeout(totalTimer)
        return chunk
      },
      () => {
        clearTimeout(totalTimer)
        controller.abort()
      },
    )
    signal?.throwIfAborted()
    return {
      handle: resources.add(stream),
      status: response.status,
      headers: decodedHeaders(response.headers),
      url: response.url,
    }
  } catch (error) {
    clearTimeout(totalTimer)
    controller.abort()
    throw error
  } finally {
    clearTimeout(timer)
    resources.remove(pending)
  }
}

function decodedHeaders (headers) {
  const decoded = headers.get('content-encoding')?.split(',')
    .every(encoding => ['gzip', 'deflate', 'br'].includes(encoding.trim().toLowerCase()))
  return [...headers].filter(([name]) => !decoded || (name !== 'content-encoding' && name !== 'content-length'))
}

async function readWithTimeout (reader, controller, timeoutMs) {
  const timer = setTimeout(() => controller.abort(new Error('HTTP response timed out')), timeoutMs)
  try {
    return await reader?.read() ?? { done: true }
  } finally {
    clearTimeout(timer)
  }
}
