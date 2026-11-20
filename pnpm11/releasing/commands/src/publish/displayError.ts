export function displayError (error: unknown): string {
  if (typeof error !== 'object' || !error) return JSON.stringify(error)

  const code = readErrorCode(error)
  const body = 'message' in error && typeof error.message === 'string' ? error.message : undefined

  if (code && body) return `${code}: ${body}`
  if (code) return code
  if (body) return body

  return JSON.stringify(error)
}

function readErrorCode (error: object): string | undefined {
  if ('code' in error && typeof error.code === 'string') return error.code
  if ('name' in error && typeof error.name === 'string') return error.name
  return undefined
}
