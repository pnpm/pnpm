export default function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    saveExact: boolean,
    savePrefix: string,
  },
) {
  const prefix = alias !== name ? `npm:${name}@` : ''
  if (opts.saveExact) return `${prefix}${version}`
  return `${prefix}${opts.savePrefix}${version}`
}
