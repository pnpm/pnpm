import { generateQrCode } from './generateQrCode.js'

/**
 * Formats the "Authenticate your account at" message for an authentication
 * URL, appending a QR code rendering of it when one can be generated.
 *
 * The URL itself is the authentication mechanism and the QR code only a
 * convenience, so a QR generation failure (e.g. a URL exceeding the maximum
 * QR data capacity) downgrades to a `globalWarn` and a URL-only message
 * instead of aborting the authentication flow.
 */
export function formatAuthUrlMessage (authUrl: string, globalWarn: (message: string) => void): string {
  let qrCode: string
  try {
    qrCode = generateQrCode(authUrl)
  } catch (err) {
    globalWarn(`Could not generate a QR code: ${String(err)}`)
    return formatAuthUrlOnlyMessage(authUrl)
  }
  return `${formatAuthUrlOnlyMessage(authUrl)}\n\n${qrCode}`
}

/**
 * Formats the "Authenticate your account at" message without a QR code — for
 * output that is not a terminal and cannot render one.
 */
export function formatAuthUrlOnlyMessage (authUrl: string): string {
  return `Authenticate your account at:\n${authUrl}`
}
