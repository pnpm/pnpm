import { PnpmError } from '@pnpm/error'

export class WebAuthTimeoutError extends PnpmError {
  readonly endTime: number
  readonly startTime: number
  readonly timeout: number
  constructor (endTime: number, startTime: number, timeout: number) {
    super('WEBAUTH_TIMEOUT', 'Web-based authentication timed out before it could be completed', {
      hint: 'Re-run this command and complete the authentication step in your browser before the time limit is reached',
    })
    this.endTime = endTime
    this.startTime = startTime
    this.timeout = timeout
  }
}
