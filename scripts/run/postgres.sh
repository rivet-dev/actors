#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="rivet-engine-postgres"
POSTGRES_IMAGE="postgres:18"

if docker ps --all --format '{{.Names}}' | grep -qw "${CONTAINER_NAME}"; then
  if docker ps --format '{{.Names}}' | grep -qw "${CONTAINER_NAME}"; then
    docker stop "${CONTAINER_NAME}" >/dev/null 2>&1 || true
  fi
  docker rm "${CONTAINER_NAME}" >/dev/null 2>&1 || true
fi

docker run \
  --detach \
  --name "${CONTAINER_NAME}" \
  --publish 5432:5432 \
  --env POSTGRES_PASSWORD=postgres \
  --env POSTGRES_USER=postgres \
  --env POSTGRES_DB=postgres \
  "${POSTGRES_IMAGE}"
