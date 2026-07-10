// Strip ANSI escape sequences and C0/C1 control characters (except tab) from
// untrusted strings (package names, license fields) before terminal output.
/* eslint-disable no-control-regex, regexp/no-obscure-range */
const ANSI_SEQUENCE = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g
const CONTROL_CHARS = /[\x00-\x08\x0a-\x1f\x7f-\x9f]/g
/* eslint-enable no-control-regex, regexp/no-obscure-range */

export function sanitizeForTerminal (s: string): string {
  return s.replace(ANSI_SEQUENCE, '').replace(CONTROL_CHARS, '')
}
