# NexQ dev container

Reopen the repo in the container (`Dev Containers: Reopen in Container`) and you get
the full toolchain for [plan.md](../plan.md): Rust for the server/CLI, Node for the
web UI, plus the tooling the plan's acceptance bars imply (`aws` CLI for the
SQS/SNS facades, `docker`/`helm` for the image and chart).

## What's in the image

| Area | Tooling |
| --- | --- |
| Rust | stable toolchain + `rustfmt`, `clippy`, `rust-src`, `rust-analyzer`, `x86_64-unknown-linux-musl` target |
| Cargo tools | `cargo-nextest`, `cargo-watch`, `cargo-deny` |
| Native deps | `protobuf-compiler` (KEDA gRPC scaler, Q16), `libssl-dev`, `libsqlite3-dev`, `musl-tools` (Q21 static build), `postgresql-client`, `sqlite3` |
| Frontend | Node LTS + npm, Angular CLI global (`ng`), React via `npm create vite@latest` — framework choice is still open (Q18) |
| Ops | `docker` (docker-in-docker), `kubectl`, `helm`, `aws` CLI, `gh`, `jq` |

## Backends

Nothing but the dev container itself starts by default — the `memory` and `sqlite`
backends need no services, and each JVM-based store costs ~1GB of RAM. Start the
networked backends only when you're working on their adapters:

```bash
docker compose -f .devcontainer/docker-compose.yml up -d postgres
docker compose -f .devcontainer/docker-compose.yml up -d opensearch
docker compose -f .devcontainer/docker-compose.yml up -d elasticsearch

# stop them again
docker compose -f .devcontainer/docker-compose.yml stop postgres opensearch
```

Connection details are pre-set in the container environment:

| Backend | Env var | Value |
| --- | --- | --- |
| Postgres | `NEXQ_POSTGRES_URL` / `DATABASE_URL` | `postgres://nexq:nexq@postgres:5432/nexq` |
| OpenSearch | `NEXQ_OPENSEARCH_URL` | `http://opensearch:9200` |
| Elasticsearch | `NEXQ_ELASTICSEARCH_URL` | `http://elasticsearch:9200` |

Both search backends run with security disabled — dev convenience only, never a
model for how NexQ is deployed.

Data lives in named volumes (`postgres-data`, `opensearch-data`,
`elasticsearch-data`), so it survives container rebuilds. To wipe it:

```bash
docker compose -f .devcontainer/docker-compose.yml down -v
```

## Ports

`8080` HTTP (REST/SQS/SNS facades) · `9090` Prometheus · `9091` KEDA external
scaler gRPC · `4200` Angular dev server · `5173` Vite dev server. Postgres and
OpenSearch are reachable by service name from inside the container; they aren't
forwarded to the host by default (add a `ports:` entry to the compose file if you
want to point a host-side client at them).

## Git over SSH

The host's `~/.ssh` is bind-mounted read-only at `~/.ssh-host`, and `post-create.sh`
copies the private keys (`id_*`, `*.pem`) and `config` into `~/.ssh` with mode 0600 —
ssh refuses keys that are group/world readable, and host bind mounts don't reliably
preserve the bits. `known_hosts` is *not* copied; the image already pins github.com.
A forwarded `SSH_AUTH_SOCK` from the host, if present, takes precedence over the
copied keys.

`post-create.sh` prints whether `git@github.com` authenticated. If it didn't:

```bash
ls -l ~/.ssh-host                    # is the host dir actually mounted?
ssh -vT git@github.com               # which key is ssh offering?
```

Passphrase-protected keys need an agent — either run `ssh-add` on the host before
opening the container (VS Code forwards the agent), or `eval $(ssh-agent) && ssh-add`
inside it. On a Windows host the mount resolves via `%USERPROFILE%\.ssh`.

## Claude Code history

`~/.claude` is the named volume `nexq-claude-data`, so transcripts, prompt history
and credentials survive `Dev Containers: Rebuild Container`. `CLAUDE_CONFIG_DIR` is
set to that same path (its default) because otherwise `~/.claude.json` — prompt
history, project trust, MCP config — would sit outside the volume.

```bash
docker volume rm nexq-claude-data    # wipe it (container must be down)
```

## Caching

`target/` and cargo's registry are named volumes rather than bind mounts — build
artifacts never cross the bind-mount boundary, which is the single biggest factor
in incremental build speed. Same for `ui/node_modules`. Consequence: `rm -rf target`
inside the container works fine, but the directory won't appear on the host.

## Testing the SQS facade with a real `aws` CLI

Per Q9/Q10 the bar is that an unmodified AWS CLI works. Once NexQ issues a
key/secret pair:

```bash
aws configure --profile nexq          # key/secret from NexQ, any region string
aws --profile nexq --endpoint-url http://localhost:8080 sqs list-queues
```
