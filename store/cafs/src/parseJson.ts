import stripBom from 'strip-bom'

export function parseJsonBufferSync (buffer: Buffer): unknown {
  return JSON.parse(stripBom(buffer.toString()))
}
