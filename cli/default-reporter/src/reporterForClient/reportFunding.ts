import { type FundingLog } from '@pnpm/core-loggers'
import chalk from 'chalk'
import * as Rx from 'rxjs'
import { map } from 'rxjs/operators'

// OSC 8 hyperlink escape sequence for clickable URLs in terminals
function terminalLink (url: string, text: string): string {
  return `\u001B]8;;${url}\u0007${chalk.cyan(text)}\u001B]8;;\u0007`
}

export function reportFunding (
  funding$: Rx.Observable<FundingLog>
): Rx.Observable<Rx.Observable<{ msg: string }>> {
  return funding$.pipe(
    map((log) => {
      let msg: string

      switch (log.fundingType) {
      case 'funding':
        msg = `${chalk.yellow('Fund')} your dependency ${chalk.bold(log.packageName)}: ${terminalLink(log.fundingUrl, log.fundingUrl)}`
        break
      case 'repository':
        msg = `${chalk.blue('Star')} your dependency ${chalk.bold(log.packageName)} on GitHub: ${terminalLink(log.fundingUrl, log.fundingUrl)}`
        break
      case 'homepage':
        msg = `${chalk.green('Support')} your dependency ${chalk.bold(log.packageName)}: ${terminalLink(log.fundingUrl, log.fundingUrl)}`
        break
      }

      return Rx.of({ msg })
    })
  )
}
