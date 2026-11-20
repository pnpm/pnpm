export function parseCAFileContents (contents: string): string[] {
  const delim = '-----END CERTIFICATE-----'
  return contents
    .split(delim)
    .filter(ca => ca.trim().length > 0)
    .map(ca => `${ca.trimStart()}${delim}`)
}
