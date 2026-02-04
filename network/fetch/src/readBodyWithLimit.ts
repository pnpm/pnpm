import { type Response } from 'node-fetch'
import { ResponseBodyTooLargeError } from './ResponseBodyTooLargeError.js'

/**
 * Read a response body into a Buffer with size limit protection.
 * Throws ResponseBodyTooLargeError if the body exceeds maxSize.
 *
 * @param response - The fetch Response object
 * @param maxSize - Maximum allowed body size in bytes
 * @param url - The URL being fetched (for error messages)
 */
export async function readBodyWithLimit (
  response: Response,
  maxSize: number,
  url: string
): Promise<Buffer> {
  // Early check: if Content-Length header indicates body is too large, reject immediately
  const contentLength = response.headers.get('content-length')
  if (contentLength != null) {
    const size = parseInt(contentLength, 10)
    if (!isNaN(size) && size > maxSize) {
      throw new ResponseBodyTooLargeError({
        url,
        maxSize,
        receivedSize: size,
      })
    }
  }

  // Read body chunks with size tracking
  const chunks: Buffer[] = []
  let received = 0

  for await (const chunk of response.body!) {
    received += (chunk as Buffer).length
    if (received > maxSize) {
      throw new ResponseBodyTooLargeError({
        url,
        maxSize,
        receivedSize: received,
      })
    }
    chunks.push(chunk as Buffer)
  }

  // Combine chunks into final buffer
  const data = Buffer.from(new SharedArrayBuffer(received))
  let offset = 0
  for (const chunk of chunks) {
    chunk.copy(data, offset)
    offset += chunk.length
  }

  return data
}

/**
 * Read a response body as JSON with size limit protection.
 * Throws ResponseBodyTooLargeError if the body exceeds maxSize.
 *
 * @param response - The fetch Response object
 * @param maxSize - Maximum allowed body size in bytes
 * @param url - The URL being fetched (for error messages)
 */
export async function readJsonWithLimit<T> (
  response: Response,
  maxSize: number,
  url: string
): Promise<T> {
  const buffer = await readBodyWithLimit(response, maxSize, url)
  return JSON.parse(buffer.toString('utf-8'))
}
