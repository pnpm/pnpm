export function createUpload (resources) {
  let controller
  const stream = new TransformStream({ start (value) { controller = value } }, { highWaterMark: 1 }, { highWaterMark: 1 })
  const writer = stream.writable.getWriter()
  return { handle: resources.add({
    kind: 'upload',
    readable: stream.readable,
    writer,
    close: () => controller.error(new Error('HTTP upload cancelled')),
  }) }
}

export async function writeUpload (upload, bytes) {
  if (Array.isArray(bytes) && bytes.every(byte => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
    bytes = Uint8Array.from(bytes)
  }
  if (!(bytes instanceof Uint8Array) || bytes.length > 65536) {
    throw new TypeError('HTTP upload chunks must contain at most 65536 bytes')
  }
  await upload.writer.write(bytes)
}
