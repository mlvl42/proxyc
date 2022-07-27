#!/bin/bash
set -e

RET=0
CONTAINER_NAME=proxyc_tests

# setup
docker build --quiet -t proxyc/tests tests/e2e
docker run --rm --name="$CONTAINER_NAME" --add-host srv.local.priv:127.0.0.1 --dns-search=local.priv -d -it proxyc/tests

sleep 1

CONTAINER_IP=$(docker inspect -f '{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$CONTAINER_NAME")

# capture tests return code to propagate it after shutting down the container
CONTAINER_IP="$CONTAINER_IP" TARGET_BIN=./target/debug/proxyc pytest tests/e2e/tests.py || RET=1

# teardown
docker kill proxyc_tests

exit $RET
