import path from 'path'
import loadJsonFile from 'load-json-file'

// eslint-disable-next-line
export const DEV_VULN_ONLY_RESP = loadJsonFile.sync<any>(path.join(__dirname, 'dev-vulnerabilities-only-response.json'))
// eslint-disable-next-line
export const ALL_VULN_RESP = loadJsonFile.sync<any>(path.join(__dirname, 'all-vulnerabilities-response.json'))
// eslint-disable-next-line
export const NO_VULN_RESP = loadJsonFile.sync<any>(path.join(__dirname, 'no-vulnerabilities-response.json'))
