#!/bin/bash
# // SPDX-License-Identifier: BUSL-1.1
# // Copyright (c) 2026 M. Javani
# //
# // This file is part of rzgate.
# //
# // Use of this software is governed by the Business Source License 1.1
# // included in the LICENSE file in the root of this repository.

# Quick local cert/key generation for testing
# Generates CA and a single localhost cert for Postman/local HTTPS

set -e

CERTS_DIR="certs"
CA_PEM="$CERTS_DIR/ca.pem"
CA_KEY="$CERTS_DIR/ca.key"
CERT_PEM="$CERTS_DIR/cert.pem"
KEY_PEM="$CERTS_DIR/key.pem"
CA_DAYS=3650
CERT_DAYS=365
KEY_BITS=2048
HOSTNAME="localhost"

mkdir -p "$CERTS_DIR"

# Generate CA if not exists
if [ ! -f "$CA_PEM" ]; then
    openssl genrsa -out "$CA_KEY" $KEY_BITS
    openssl req -new -x509 -days $CA_DAYS -key "$CA_KEY" -out "$CA_PEM" -subj "/CN=Local Test CA" -sha256
    echo "Generated CA: $CA_PEM"
fi

# Generate server key
openssl genrsa -out "$KEY_PEM" $KEY_BITS

# Generate CSR
openssl req -new -key "$KEY_PEM" -out "$CERTS_DIR/temp.csr" -subj "/CN=$HOSTNAME" -sha256

# Sign with CA, add SAN
openssl x509 -req -in "$CERTS_DIR/temp.csr" \
    -CA "$CA_PEM" -CAkey "$CA_KEY" -CAcreateserial \
    -out "$CERT_PEM" -days $CERT_DAYS -sha256 \
    -extfile <(printf "subjectAltName=DNS:%s" "$HOSTNAME")

rm "$CERTS_DIR/temp.csr"

echo "Generated localhost cert and key:"
echo "  $CERT_PEM"
echo "  $KEY_PEM"
echo "Use $CA_PEM as trusted CA in Postman or browser."
