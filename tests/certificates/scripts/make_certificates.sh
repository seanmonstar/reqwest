function mk_skeleton() {
    local root=${1}

    rm -rf ${root}
    mkdir -p \
        ${root} \
        ${root}/certs \
        ${root}/crl \
        ${root}/newcerts \
        ${root}/private

    # limite permissions on the directory in which we put the private keys
    chmod 700 ${root}/private

    # these two files are used by openssl to keep track of the signed certificates
    touch ${root}/index.{txt.attr,txt}
    echo 1000 > ${root}/serial
}

function mk_certificate_authorities {
    # Create the directory structure
    mk_skeleton ${ROOT}
    mk_skeleton ${INTERMEDIATE}
    mkdir ${INTERMEDIATE}/csr
    echo 1000 > ${INTERMEDIATE}/crlnumber

    # Create the openssl config file for each CA
    cat ${OPENSSL_CONFIG_TPL} | \
        sed "s|CHANGE_ME_DIR|${ROOT}|g" | \
        sed "s|CHANGE_ME_POLICY|policy_strict|g" | \
        sed "s|CHANGE_ME_OBJECTS|ca|g" \
        > ${ROOT}/openssl.cnf

    cat ${OPENSSL_CONFIG_TPL} | \
        sed "s|CHANGE_ME_DIR|${INTERMEDIATE}|g" | \
        sed "s|CHANGE_ME_POLICY|policy_loose|g" | \
        sed "s|CHANGE_ME_OBJECTS|intermediate|g" \
        > ${INTERMEDIATE}/openssl.cnf

    # Create the root CA private key
    openssl genrsa \
        -aes256 \
        -out ${ROOT}/private/ca.key.pem \
        -passout pass:mypass \
        4096
    chmod 400 ${ROOT}/private/ca.key.pem

    # Create the root CA certificate (self-signed)
    openssl req  \
        -new \
        -x509 \
        -config ${ROOT}/openssl.cnf \
        -days 7300 \
        -sha256 \
        -extensions v3_ca \
        -key ${ROOT}/private/ca.key.pem \
        -passin pass:mypass \
        -out ${ROOT}/certs/ca.cert.pem \
        -subj "/C=ZZ/ST=Some State/L=Some City/O=Some Organization/CN=Some Root CA"
    chmod 444 ${ROOT}/certs/ca.cert.pem

    # Create the intermediate CA private key
    openssl genrsa \
        -aes256 \
        -out ${INTERMEDIATE}/private/intermediate.key.pem \
        -passout pass:mypass \
        4096
    chmod 400 ${INTERMEDIATE}/private/intermediate.key.pem

    # Create the certificate signing request
    # We keep the same C, ST, L and O that the root CA, but we have to change the CN.
    openssl req \
        -new \
        -sha256 \
        -config ${INTERMEDIATE}/openssl.cnf \
        -key ${INTERMEDIATE}/private/intermediate.key.pem \
        -passin pass:mypass \
        -out ${INTERMEDIATE}/csr/intermediate.csr.pem \
        -subj "/C=ZZ/ST=Some State/L=Some City/O=Some Organization/CN=Some intermediate CA"

    # Sign the certificate request to make an intermediate cert.
    openssl ca \
        -config ${ROOT}/openssl.cnf \
        -extensions v3_intermediate_ca \
        -days 3650 \
        -notext \
        -md sha256 \
        -in ${INTERMEDIATE}/csr/intermediate.csr.pem \
        -out ${INTERMEDIATE}/certs/intermediate.cert.pem \
        -passin pass:mypass \
        -batch
    chmod 444 ${INTERMEDIATE}/certs/intermediate.cert.pem
}

function mk_server_certificate() {
    local cn=${1}

    # Create a server private key if it does not already exist
    if [ ! -f ${INTERMEDIATE}/private/server.key.pem ] ; then
        openssl genrsa \
            -aes256 \
            -passout pass:mypass \
            -out ${INTERMEDIATE}/private/server.key.pem \
            2048
    fi

    openssl req \
        -new \
        -sha256 \
        -config ${INTERMEDIATE}/openssl.cnf \
        -key ${INTERMEDIATE}/private/server.key.pem \
        -passin pass:mypass \
        -out ${INTERMEDIATE}/csr/${cn}.csr.pem \
        -subj "/C=XX/ST=Some Other State/L=Some Other City/O=Some Other Organization/CN=${cn}"

    openssl ca \
        -config ${INTERMEDIATE}/openssl.cnf \
        -extensions server_cert \
        -days 365 \
        -notext \
        -md sha256 \
        -in ${INTERMEDIATE}/csr/${cn}.csr.pem \
        -out ${INTERMEDIATE}/certs/${cn}.cert.pem \
        -passin pass:mypass \
        -batch
    chmod 444 ${INTERMEDIATE}/certs/${cn}.cert.pem
}

function mk_client_test_certificate() {
    # all the client need is a DER version of the root certificate
    openssl x509 -outform der -in ${ROOT}/certs/ca.cert.pem -out ${WS}/root.der
}

function mk_server_test_certificate() {
    local cn=${1}

    # Create a bundle with both the root and intermediate certificate.
    if [ ! -f "${INTERMEDIATE}/certs/ca-chain.cert.pem" ] ; then
        cat \
            ${INTERMEDIATE}/certs/intermediate.cert.pem \
            ${ROOT}/certs/ca.cert.pem \
            > ${INTERMEDIATE}/certs/ca-chain.cert.pem
        chmod 444 ${INTERMEDIATE}/certs/ca-chain.cert.pem
    fi

    # Create a PKCS #12 of that contains the server private key, the ca chain
    # certificate, and the server certificate.
    openssl pkcs12 \
        -export \
        -out ${WS}/server-${cn}.pfx\
        -inkey ${INTERMEDIATE}/private/server.key.pem \
        -certfile ${INTERMEDIATE}/certs/ca-chain.cert.pem \
        -in ${INTERMEDIATE}/certs/${cn}.cert.pem \
        -passin pass:mypass \
        -passout pass:mypass
}

set -e
set -x

WS=${1}
ROOT=${WS}/ca
INTERMEDIATE=${ROOT}/intermediate
OPENSSL_CONFIG_TPL=${WS}/openssl.cnf.tpl

mk_certificate_authorities
mk_server_certificate localhost
mk_server_certificate wrong.hostname.com

mk_client_test_certificate
mk_server_test_certificate localhost
mk_server_test_certificate wrong.hostname.com
