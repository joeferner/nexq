#!/usr/bin/env bash
set -euo pipefail

echo "==> Fixing ownership of mounted volumes"
sudo mkdir -p /workspaces/nexq/target /workspaces/nexq/ui/node_modules
sudo chown -R "$(id -u):$(id -g)" \
    /workspaces/nexq/target \
    /workspaces/nexq/ui \
    /usr/local/cargo/registry

echo "==> Installing frontend tooling (Angular CLI; React via 'npm create vite@latest')"
npm install -g @angular/cli >/dev/null

echo "==> Toolchain versions"
rustc --version
cargo --version
node --version
npm --version
ng version --help >/dev/null 2>&1 && echo "ng $(ng version 2>/dev/null | grep -oP 'Angular CLI: \K.*' || echo installed)"
aws --version
helm version --short
docker --version

cat <<'EOF'

==> NexQ dev container ready.

Backends: nothing extra is running. The memory and sqlite backends need no
services. When you need a networked backend (postgres/opensearch/elasticsearch):

  docker compose -f .devcontainer/docker-compose.yml up -d postgres
  docker compose -f .devcontainer/docker-compose.yml up -d opensearch
  docker compose -f .devcontainer/docker-compose.yml up -d elasticsearch

Connection strings are already in the environment as NEXQ_POSTGRES_URL,
NEXQ_OPENSEARCH_URL and NEXQ_ELASTICSEARCH_URL. See .devcontainer/README.md.
EOF
