import stripBom from 'strip-bom'

export function parseJsonBufferSync<T>(buffer: Buffer): T {
  return JSON.parse(stripBom(buffer.toString())) as T
}
