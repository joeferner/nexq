
GREEN='\033[0;32m'
NC='\033[0m' # No Color

function banner() {
    local message=$1
    echo -e "${GREEN}${message}"
    echo -e "===========================================================${NC}"
}