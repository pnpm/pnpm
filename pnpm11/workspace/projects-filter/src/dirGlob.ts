const DRIVE_LETTER_PATH = /^[a-z]:[\\/]/i

// Windows drive letters are case-insensitive, but micromatch compares them
// literally. A selector resolved against a lowercase `c:\` cwd must still
// match project dirs read as `C:\`.
export function formatDirGlob (pattern: string): string {
  return upperCaseDriveLetter(pattern.replace(/\\/g, '/').replace(/\/$/, ''))
}

export function formatDirGlobCandidate (dir: string): string {
  return DRIVE_LETTER_PATH.test(dir) ? upperCaseDriveLetter(dir.replace(/\\/g, '/')) : dir
}

function upperCaseDriveLetter (dir: string): string {
  return DRIVE_LETTER_PATH.test(dir) ? `${dir[0].toUpperCase()}${dir.slice(1)}` : dir
}
