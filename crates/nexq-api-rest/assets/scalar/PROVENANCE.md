# Vendored: Scalar API Reference

`standalone.js` is a third-party build, committed rather than fetched.

| | |
| --- | --- |
| Package | [`@scalar/api-reference`](https://www.npmjs.com/package/@scalar/api-reference) |
| Version | 1.66.1 |
| Source | <https://github.com/scalar/scalar>, `packages/api-reference` |
| File | `dist/browser/standalone.js` from the published package |
| License | MIT — see [LICENSE](LICENSE) |
| Size | 3.8 MB (1.1 MB gzipped over the wire) |

## Why it is committed

Every documented alternative loads this bundle from a CDN, which an air-gapped
deployment cannot reach — the plan's Q21 requires that the image pull nothing at
runtime. A docs page that renders blank in the environments NexQ is built for would be
worse than not having one.

Committed rather than downloaded during the build for the same reason: `cargo build`
must not need the network, and a build that fetches a moving `latest` is a build whose
output nobody can reproduce.

## Refreshing it

```sh
npm pack @scalar/api-reference@<version>          # or: npm install, then copy from node_modules
tar -xzOf scalar-api-reference-<version>.tgz package/dist/browser/standalone.js \
  > crates/nexq-api-rest/assets/scalar/standalone.js
```

Then update the version above and re-run `cargo test -p nexq-api-rest`, which checks the
things this page depends on rather than trusting the new bundle:

- `the_vendored_bundle_is_self_contained` — no `chunks/` references, so nothing is fetched
  at runtime. A build that split itself into chunks would 404 half of itself.
- `the_vendored_bundle_needs_no_eval` — no `eval`, `new Function`, or `Worker`, which is
  what lets the Content-Security-Policy stay strict. If a future version needs one of
  them, the page goes blank until the policy is loosened, and that should be a decision
  rather than a surprise.
- `the_bundle_exposes_the_api_the_page_calls` — `Scalar.createApiReference` still exists.

## Two things about this bundle worth knowing

**It defaults to routing "try it" requests through `https://proxy.scalar.com`.** That
would send whatever bearer token a developer pastes into the page to a third party.
`docs-bootstrap.js` sets `proxyUrl: ''`, and the page's `connect-src 'self'` refuses the
request even if a future version renames that option.

**It fetches webfonts from `https://fonts.scalar.com` by default.** Switched off with
`withDefaultFonts: false`, and blocked by `font-src` regardless, so the page uses system
fonts instead of failing to load and waiting.
