#!/usr/bin/env bash
set -euo pipefail

echo "==> Fixing ownership of mounted volumes"
sudo mkdir -p /workspaces/nexq/target /workspaces/nexq/ui/node_modules
sudo chown -R "$(id -u):$(id -g)" \
    /workspaces/nexq/target \
    /workspaces/nexq/ui \
    /usr/local/cargo/registry

echo "==> Claude Code state"
# The volume is mounted before this runs, so fix ownership here too — a volume
# that already existed from an older image can still be root-owned.
sudo chown -R "$(id -u):$(id -g)" /home/vscode/.claude
echo "    persisted in the 'nexq-claude-data' volume (CLAUDE_CONFIG_DIR)"

echo "==> SSH"
if [ -d /home/vscode/.ssh-host ]; then
    mkdir -p ~/.ssh
    # Copy rather than mount over ~/.ssh directly: the keys need mode 0600 (host
    # bind mounts don't always land there), and the image's known_hosts — which
    # already pins github.com — stays intact.
    find /home/vscode/.ssh-host -maxdepth 1 -type f \
        \( -name 'id_*' -o -name '*.pem' -o -name 'config' \) \
        -exec cp -f {} ~/.ssh/ \;
    chmod 700 ~/.ssh
    chmod 600 ~/.ssh/* 2>/dev/null || true
    chmod 644 ~/.ssh/*.pub 2>/dev/null || true
    # UseKeychain is macOS-only; OpenSSH on Linux aborts on the unknown keyword.
    if [ -f ~/.ssh/config ]; then
        sed -i '/[Uu]se[Kk]eychain/d' ~/.ssh/config
    fi
    keys=$(find ~/.ssh -maxdepth 1 -type f -name 'id_*' ! -name '*.pub' | wc -l)
    echo "    copied $keys private key(s) from the host"
else
    echo "    no host ~/.ssh mounted"
fi
if [ -n "${SSH_AUTH_SOCK:-}" ]; then
    echo "    ssh-agent forwarded from host"
fi
# `ssh -T git@github.com` exits 1 even when auth succeeds, so match on the banner
# instead of the status (and keep the non-zero exit away from `set -o pipefail`).
ssh_probe=$(ssh -o BatchMode=yes -o ConnectTimeout=10 -T git@github.com 2>&1 || true)
if grep -q 'successfully authenticated' <<<"$ssh_probe"; then
    echo "    github.com: authenticated"
else
    echo "    github.com: NOT authenticated (see .devcontainer/README.md)"
fi

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
