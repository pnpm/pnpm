# @pnpm/exe

This version of the pnpm CLI is packaged with Node.js into an executable.
So it may be used on a system with no Node.js installed.
This makes pnpm not only a Node.js package manager but also a Node.js version manager (see [related discussion](https://github.com/pnpm/pnpm/discussions/3434)).

## Installation

On macOS, Linux, or Windows Subsystem for Linux:

```
curl -fsSL https://get.pnpm.io/install.sh | sh -
```

If you don't have curl installed, you would like to use wget:

```
wget -qO- https://get.pnpm.io/install.sh | sh -
```

After installation, restart your shell to get pnpm accessible.

### Alternatively, if you do have Node.js installed

On macOS, Linux, or Windows Subsystem for Linux:

```
curl -f https://get.pnpm.io/v6.16.js | node - add --global @pnpm/exe
```

On Windows (using PowerShell):

```
(Invoke-WebRequest 'https://get.pnpm.io/v6.16.js' -UseBasicParsing).Content | node - add --global @pnpm/exe
```

## License

MIT
