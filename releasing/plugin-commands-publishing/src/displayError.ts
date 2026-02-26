export function displayError (error: unknown): string {
  if (typeof error !== 'object' || !error) return JSON.stringify(error)

  let code: string | undefined
  let body: string | undefined

  if ('code' in error && typeof error.code === 'string') {
    code = error.code
  } else if ('name' in error && typeof error.name === 'string') {
    code = error.name
  }

  if ('message' in error && typeof error.message === 'string') {
    body = error.message
  }

  if (code && body) return `${code}: ${body}`
  if (code) return code
  if (body) return body

  return JSON.stringify(error)
}
