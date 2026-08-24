#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

CLUSTER_NAME="rivet"
IMAGE_NAME="rivet-engine:local"
NAMESPACE="rivet-engine"

# Check required commands
for cmd in k3d kubectl docker; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: required command '$cmd' not found."
    exit 1
  fi
done

# Create cluster if it doesn't exist
if ! k3d cluster list | grep -qw "^${CLUSTER_NAME} "; then
  echo "Creating k3d cluster '${CLUSTER_NAME}'..."
  k3d cluster create "${CLUSTER_NAME}" \
    --api-port 6550 \
    -p "6420:30420@loadbalancer" \
    -p "6421:30421@loadbalancer" \
    --agents 2
fi

# Build image
echo "Building image..."
cd "${REPO_ROOT}"
docker build -f docker/engine/Dockerfile -t "${IMAGE_NAME}" .

# Load image into k3d
echo "Loading image into k3d..."
k3d image import "${IMAGE_NAME}" -c "${CLUSTER_NAME}"

# Deploy
echo "Deploying to Kubernetes..."
cd "${REPO_ROOT}/self-host/control-plane/kubernetes"

kubectl apply -f 00-namespace.yaml
kubectl apply -f 01-serviceaccount.yaml
kubectl apply -f 06-nats-configmap.yaml
kubectl apply -f 07-nats-statefulset.yaml
kubectl apply -f 08-nats-pdb.yaml
kubectl apply -f 09-nats-service.yaml
kubectl apply -f 10-postgres-configmap.yaml
kubectl apply -f 11-postgres-secret.yaml
kubectl apply -f 12-postgres-statefulset.yaml
kubectl apply -f 13-postgres-service.yaml

# Wait for NATS and PostgreSQL to be ready
echo "Waiting for NATS to be ready..."
kubectl -n "${NAMESPACE}" wait --for=condition=ready pod -l app=nats --timeout=300s
echo "Waiting for PostgreSQL to be ready..."
kubectl -n "${NAMESPACE}" wait --for=condition=ready pod -l app=postgres --timeout=300s

kubectl apply -f 02-engine-configmap.yaml
kubectl apply -f 03-rivet-engine-deployment.yaml
kubectl apply -f 04-rivet-engine-service.yaml
kubectl apply -f 05-rivet-engine-hpa.yaml
kubectl apply -f 14-rivet-engine-pdb.yaml

# Override image to use locally built image
kubectl -n "${NAMESPACE}" set image deployment/rivet-engine rivet-engine="${IMAGE_NAME}"
kubectl -n "${NAMESPACE}" patch deployment rivet-engine -p '{"spec":{"template":{"spec":{"containers":[{"name":"rivet-engine","imagePullPolicy":"Never"}]}}}}'

# Wait for engine to be ready
echo "Waiting for engine to be ready..."
kubectl -n "${NAMESPACE}" wait --for=condition=ready pod -l app=rivet-engine --timeout=300s

echo ""
echo "Deployment complete."
echo ""
echo "Access the engine at:"
echo "  http://localhost:6420 (guard)"
echo "  http://localhost:6421 (api-peer)"
echo ""
echo "Useful commands:"
echo "  kubectl -n ${NAMESPACE} get pods"
echo "  kubectl -n ${NAMESPACE} logs -l app=rivet-engine -c rivet-engine -f"
echo "  k3d cluster delete ${CLUSTER_NAME}"
echo ""
