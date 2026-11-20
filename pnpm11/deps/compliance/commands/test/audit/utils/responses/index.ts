import path from 'node:path'

import { loadJsonFileSync } from 'load-json-file'

export const DEV_VULN_ONLY_RESP = loadJsonFileSync<Record<string, unknown>>(path.join(import.meta.dirname, 'dev-vulnerabilities-only-response.json'))
export const ALL_VULN_RESP = loadJsonFileSync<Record<string, unknown>>(path.join(import.meta.dirname, 'all-vulnerabilities-response.json'))
export const NO_VULN_RESP = loadJsonFileSync<Record<string, unknown>>(path.join(import.meta.dirname, 'no-vulnerabilities-response.json'))
export const INFO_VULN_RESP = loadJsonFileSync<Record<string, unknown>>(path.join(import.meta.dirname, 'info-vulnerability-response.json'))
