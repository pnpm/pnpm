export function getPinnedVersion (opts: { saveExact?: boolean, savePrefix?: string }): 'major' | 'minor' | 'patch' {
  if (opts.saveExact === true || opts.savePrefix === '') return 'patch'
  return opts.savePrefix === '~' ? 'minor' : 'major'
}
