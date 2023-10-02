import stripBom from 'strip-bom'

export function parseJsonBufferSync (buffer: Buffer) {
  return JSON.parse(stripBom(buffer.toString()))
}
