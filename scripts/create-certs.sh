#!/bin/bash
set -u
set -e

DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
cd "${DIR}/../packages/app"

CANAME=nexq-rootca
MYCERT=nexq.local

# optional, create a directory
mkdir -p certs
cd certs

# generate aes encrypted private key
echo "creating ca private key"
openssl genrsa -aes256 -out $CANAME.key -passout pass:password 4096

# create certificate, 1826 days = 5 years
echo "creating ca cert"
openssl req \
  -x509 \
  -new \
  -nodes \
  -key $CANAME.key \
  -passin pass:password \
  -sha256 \
  -days 1826 \
  -out $CANAME.crt \
  -subj '/CN=NexQ Root CA/C=AT/ST=Vienna/L=Vienna/O=NexQOrg'

# create certificate for server
echo "creating server cert request"
openssl req \
  -new \
  -nodes \
  -out $MYCERT.csr \
  -newkey rsa:4096 \
  -keyout $MYCERT.key \
  -subj '/CN=nexq.local/C=AT/ST=Vienna/L=Vienna/O=NexQOrg'

# create a v3 ext file for SAN properties
echo "creating server cert"
cat > $MYCERT.v3.ext << EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
subjectAltName = @alt_names

[alt_names]
DNS.1 = nexq.local
EOF

openssl x509 \
  -req \
  -in $MYCERT.csr \
  -CA $CANAME.crt \
  -CAkey $CANAME.key \
  -passin pass:password \
  -CAcreateserial \
  -out $MYCERT.crt \
  -days 730 \
  -sha256 \
  -extfile $MYCERT.v3.ext

echo "Complete!"
