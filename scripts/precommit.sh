#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

banner "checking for invalid code..."
if grep -R --exclude-dir=node_modules 'import.*from.*/build' packages; then
  echo "should not import from /build"
  exit 1
fi

banner "Installing..."
npm install --workspaces --no-save

banner "Building..."
npm run build --workspaces

banner "Formatting..."
npm run format --workspaces
npm run lint --workspaces

banner "Testing..."
npm run test --workspaces --if-present

echo "complete!"
