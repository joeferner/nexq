#!/bin/bash
set -e
set -u

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${SCRIPT_DIR}/.."

source ./scripts/_utils.sh

banner "Formatting..."
./scripts/format.sh

banner "Building..."
./scripts/build.sh

banner "Testing..."
./scripts/test.sh

echo "complete!"
