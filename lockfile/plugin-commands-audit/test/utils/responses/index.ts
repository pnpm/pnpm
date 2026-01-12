import path from 'path'
import { loadJsonFileSync } from 'load-json-file'

// eslint-disable-next-line
export const DEV_VULN_ONLY_RESP = loadJsonFileSync<any>(path.join(import.meta.dirname, 'dev-vulnerabilities-only-response.json'))
// eslint-disable-next-line
export const ALL_VULN_RESP = loadJsonFileSync<any>(path.join(import.meta.dirname, 'all-vulnerabilities-response.json'))
// eslint-disable-next-line
export const NO_VULN_RESP = loadJsonFileSync<any>(path.join(import.meta.dirname, 'no-vulnerabilities-response.json'))
