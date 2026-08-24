#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="rivet-engine-postgres"
POSTGRES_IMAGE="postgres:18"

if [ $# -ne 1 ]; then
	echo "Usage: $0 <backup-path>"
	echo "Example: $0 /path/to/backup/postgres-backup.tar.gz"
	exit 1
fi

BACKUP_PATH="$1"

if [ ! -f "${BACKUP_PATH}" ]; then
	echo "error: backup file '${BACKUP_PATH}' not found"
	exit 1
fi

# Stop and remove existing container if it exists
if docker ps --all --format '{{.Names}}' | grep -qw "${CONTAINER_NAME}"; then
	echo "Stopping and removing existing container '${CONTAINER_NAME}'..."
	if docker ps --format '{{.Names}}' | grep -qw "${CONTAINER_NAME}"; then
		docker stop "${CONTAINER_NAME}" >/dev/null 2>&1 || true
	fi
	docker rm "${CONTAINER_NAME}" >/dev/null 2>&1 || true
fi

# Create a new volume
echo "Creating new volume..."
VOLUME_NAME=$(docker volume create)
echo "Created volume: ${VOLUME_NAME}"

# Restore the data to the new volume
echo "Restoring data from '${BACKUP_PATH}'..."
BACKUP_DIR="$(dirname "${BACKUP_PATH}")"
docker run --rm \
	-v "${VOLUME_NAME}":/data \
	-v "${BACKUP_DIR}":/backup \
	alpine \
	tar xzf "/backup/$(basename "${BACKUP_PATH}")" -C /data

# Create and start the container
echo "Starting container '${CONTAINER_NAME}'..."
docker run \
	--detach \
	--name "${CONTAINER_NAME}" \
	--publish 5432:5432 \
	--env POSTGRES_PASSWORD=postgres \
	--env POSTGRES_USER=postgres \
	--env POSTGRES_DB=postgres \
	-v "${VOLUME_NAME}":/var/lib/postgresql/data \
	"${POSTGRES_IMAGE}"

echo "Restore completed successfully!"
echo "Container ID: $(docker ps -q -f name=${CONTAINER_NAME})"
