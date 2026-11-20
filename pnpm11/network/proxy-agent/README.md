# @pnpm/network.proxy-agent

> Creates an http/s/socks proxy agent

Create a http/s/socks proxy agent from uri and options.
This is usually used from within the `agent` component, to create an agent or proxy agent according to the config.

The return agent will match the proxy protocol (http / https / socks).

API:
```js
function getProxyAgent(uri: string, opts: AgentOptions)
```

Available configuration (AgentOptions):

```js
  /**
   * A proxy server for out going network requests by the package manager
   * Used for both http and https requests (unless the httpsProxy is defined)
   */
  httpProxy?: string;

  /**
   * A proxy server for outgoing https requests by the package manager (fallback to proxy server if not defined)
   * Use this in case you want different proxy for http and https requests.
   */
  httpsProxy?: string;

  /**
   * A path to a file containing one or multiple Certificate Authority signing certificates.
   * allows for multiple CA's, as well as for the CA information to be stored in a file on disk.
   */
  ca?: string;

  /**
   * Whether or not to do SSL key validation when making requests to the registry via https
   */
  strictSsl?: string;

  /**
   * A client certificate to pass when accessing the registry. Values should be in PEM format (Windows calls it "Base-64 encoded X.509 (.CER)") with newlines replaced by the string "\n". For example:
   * cert="-----BEGIN CERTIFICATE-----\nXXXX\nXXXX\n-----END CERTIFICATE-----"
   * It is not the path to a certificate file (and there is no "certfile" option).
   */
  cert?: string;

  /**
   * A client key to pass when accessing the registry. Values should be in PEM format with newlines replaced by the string "\n". For example:
   * key="-----BEGIN PRIVATE KEY-----\nXXXX\nXXXX\n-----END PRIVATE KEY-----"
   * It is not the path to a key file (and there is no "keyfile" option).
   */
  key?: string;
```

## License

MIT
