# @pnpm/network.agent

> An http/s agent factory with proxy support

Create an http/s agent from uri and options.
This support http/s proxy server with different configuration.

In case there is matching proxy option that matching the uri protocol (`httpProxy` for `http:` uri and `httpsProxy` for `https:` uri), and the uri is not excluded by the `noProxy` config, it will create a proxy agent.
To read more about the proxy agent, read the docs of the `proxy-agent` component.

API:

```js
function getAgent(uri: string, opts: AgentOptions)
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

  /**
   * A comma-separated string of domain extensions that a proxy should not be used for.
   */
  noProxy?: string;

  /**
   * The IP address of the local interface to use when making connections to the npm registry.
   */
  localAddress?: string;
```

## License

MIT
