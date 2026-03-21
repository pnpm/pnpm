import qrcodeTerminal from 'qrcode-terminal'

export function generateQrCode (text: string): string {
  let qrCode: string | undefined
  qrcodeTerminal.generate(text, { small: true }, (code: string) => {
    qrCode = code
  })
  if (qrCode != null) return qrCode
  /* istanbul ignore next */
  throw new Error('we were expecting qrcode-terminal to be fully synchronous, but it fails to execute the callback')
}
