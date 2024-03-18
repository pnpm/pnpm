export function getPinnedVersion(opts: {
  saveExact?: boolean | undefined
  savePrefix?: string | undefined
}): 'major' | 'minor' | 'patch' {
  if (opts.saveExact === true || opts.savePrefix === '') return 'patch'
  return opts.savePrefix === '~' ? 'minor' : 'major'
}
