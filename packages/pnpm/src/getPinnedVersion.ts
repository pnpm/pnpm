export default (opts: { saveExact?: boolean, savePrefix?: string }) => {
  if (opts.saveExact === true) return 'patch'
  return opts.savePrefix === '~' ? 'minor' : 'major'
}
