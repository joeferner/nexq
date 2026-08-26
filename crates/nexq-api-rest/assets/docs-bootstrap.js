// Starts the vendored Scalar bundle against this server's own OpenAPI document.
//
// Its own file rather than an inline <script> so the page's Content-Security-Policy can
// say `script-src 'self'` with no `unsafe-inline`.
//
// {{PREFIX}} is substituted when the router is built, from the same constant the routes
// are registered under.
Scalar.createApiReference('#app', {
  url: '{{PREFIX}}/openapi.json',

  // Scalar defaults this to `https://proxy.scalar.com` and routes "try it" requests
  // through it. A developer pasting a bearer token into this page would be handing it to
  // a third party, so it is emptied — which this bundle reads as "call the API directly".
  // The page's `connect-src 'self'` is the backstop if a future version renames the key.
  proxyUrl: '',

  // Scalar otherwise fetches webfonts from `https://fonts.scalar.com`. Nothing an
  // air-gapped deployment can reach, and a font that never arrives is a page that waits
  // for it, so system fonts it is.
  withDefaultFonts: false,

  // This server has one document, so its own picker would be a control with one option.
  hideDarkModeToggle: false,
  metaData: {
    title: 'NexQ API',
  },
});
