# @pnpm/beta

> This is an experimental pnpm CLI

This version of the pnpm CLI is packaged with Node.js into an executable.
So it may be used on a system with no Node.js installed.
This makes pnpm not only a Node.js package manager but also a Node.js version manager (see [related discussion](https://github.com/pnpm/pnpm/discussions/3434)).

## Installation

On macOS, Linux, or Windows Subsystem for Linux:

```
curl https://get.pnpm.io/beta-install.sh | bash
```

After installation, restart your shell to get pnpm accessible.

### Alternatively, if you do have Node.js installed

On macOS, Linux, or Windows Subsystem for Linux:

```
curl -f https://get.pnpm.io/v6.js | node - add --global @pnpm/beta
```

On Windows (using PowerShell):

```
(Invoke-WebRequest 'https://get.pnpm.io/v6.js' -UseBasicParsing).Content | node - add --global @pnpm/beta
```

## License

MIT
