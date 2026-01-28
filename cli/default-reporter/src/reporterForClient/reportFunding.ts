import { type FundingLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'
import chalk from 'chalk'

// OSC 8 hyperlink escape sequence for clickable URLs in terminals
function terminalLink (url: string): string {
  return `\u001B]8;;${url}\u0007${chalk.cyan(url)}\u001B]8;;\u0007`
}

export function reportFunding (
  funding$: Rx.Observable<FundingLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return funding$.pipe(
    map((log) => {
      const messages: string[] = []

      switch (log.fundingType) {
      case 'funding':
        messages.push(chalk.bgYellow.black(' FUND ') + ` ${chalk.bold(log.packageName)} is looking for funding`)
        if (log.packageDescription) {
          messages.push(`      ${chalk.dim(log.packageDescription)}`)
        }
        messages.push(`      ${terminalLink(log.fundingUrl)}`)
        break
      case 'repository':
        messages.push(chalk.bgBlue.black(' STAR ') + ` Please star ${chalk.bold(log.packageName)} on GitHub`)
        if (log.packageDescription) {
          messages.push(`      ${chalk.dim(log.packageDescription)}`)
        }
        messages.push(`      ${terminalLink(log.fundingUrl)}`)
        break
      case 'homepage':
        messages.push(chalk.bgGreen.black(' SUPPORT ') + ` Check out ${chalk.bold(log.packageName)}`)
        if (log.packageDescription) {
          messages.push(`      ${chalk.dim(log.packageDescription)}`)
        }
        messages.push(`      ${terminalLink(log.fundingUrl)}`)
        break
      }

      return Rx.of({ msg: messages.join('\n') })
    })
  )
}
