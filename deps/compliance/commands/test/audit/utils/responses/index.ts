import path from 'node:path'

import { loadJsonFileSync } from 'load-json-file'

// eslint-disable-next-line
const DEV_VULN_ONLY_AUDIT_REPORT = loadJsonFileSync<any>(path.join(import.meta.dirname, 'dev-vulnerabilities-only-response.json'))
// eslint-disable-next-line
const ALL_VULN_AUDIT_REPORT = loadJsonFileSync<any>(path.join(import.meta.dirname, 'all-vulnerabilities-response.json'))
// eslint-disable-next-line
const NO_VULN_AUDIT_REPORT = loadJsonFileSync<any>(path.join(import.meta.dirname, 'no-vulnerabilities-response.json'))

// The fixtures above are stored in the legacy /audits response shape because
// that is what upstream tooling and past mocks captured. The audit client now
// hits /advisories/bulk first, so convert the fixtures to bulk shape on load.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function toBulkResponse (auditReport: any): Record<string, any[]> {
  const result: Record<string, any[]> = {} // eslint-disable-line @typescript-eslint/no-explicit-any
  for (const advisory of Object.values<any>(auditReport.advisories ?? {})) { // eslint-disable-line @typescript-eslint/no-explicit-any
    const moduleName = advisory.module_name as string
    if (!result[moduleName]) result[moduleName] = []
    result[moduleName].push(advisory)
  }
  return result
}

export const DEV_VULN_ONLY_RESP = toBulkResponse(DEV_VULN_ONLY_AUDIT_REPORT)
export const ALL_VULN_RESP = toBulkResponse(ALL_VULN_AUDIT_REPORT)
export const NO_VULN_RESP = toBulkResponse(NO_VULN_AUDIT_REPORT)
