export default function getPinnedVersion (opts: { saveExact?: boolean, savePrefix?: string }) {
  if (opts.saveExact === true || opts.savePrefix === '') return 'patch'
  return opts.savePrefix === '~' ? 'minor' : 'major'
}
