#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

cd "${SCRIPT_DIR}/../"
npm install --workspaces --no-save
npm run test --workspaces --if-present
