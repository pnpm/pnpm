import path from 'path'
import { syncJSON } from '@pnpm/file-reader'

// eslint-disable-next-line
export const DEV_VULN_ONLY_RESP = syncJSON<any>(path.join(__dirname, 'dev-vulnerabilities-only-response.json'))
// eslint-disable-next-line
export const ALL_VULN_RESP = syncJSON<any>(path.join(__dirname, 'all-vulnerabilities-response.json'))
// eslint-disable-next-line
export const NO_VULN_RESP = syncJSON<any>(path.join(__dirname, 'no-vulnerabilities-response.json'))
