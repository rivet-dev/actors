# Rivet Engine Kubernetes Deployment

Production-ready Kubernetes manifests for deploying Rivet Engine with high availability.

These manifests deploy a fully HA stack: 2+ engine replicas with HPA auto-scaling, a 2-node NATS cluster, and PostgreSQL. Pod Disruption Budgets ensure safe rolling updates and node maintenance.

## Quick Start

**For deployment instructions, see the [Kubernetes Self-Hosting Guide](https://rivet.dev/actors/self-host/control-plane/kubernetes).**

## What's in This Directory

The `engine/` directory contains Kubernetes manifests:

- `00-namespace.yaml` - Namespace definition
- `01-serviceaccount.yaml` - Service account
- `02-engine-configmap.yaml` - Engine configuration
- `03-rivet-engine-deployment.yaml` - Engine deployment (2+ replicas with HPA)
- `04-rivet-engine-service.yaml` - Service definition
- `05-rivet-engine-hpa.yaml` - Horizontal Pod Autoscaler (2-10 replicas)
- `06-nats-configmap.yaml` - NATS configuration
- `07-nats-statefulset.yaml` - NATS cluster (2-node HA)
- `08-nats-pdb.yaml` - NATS Pod Disruption Budget
- `09-nats-service.yaml` - NATS headless service
- `10-postgres-configmap.yaml` - PostgreSQL configuration
- `11-postgres-secret.yaml` - PostgreSQL credentials
- `12-postgres-statefulset.yaml` - PostgreSQL database
- `13-postgres-service.yaml` - PostgreSQL service
- `14-rivet-engine-pdb.yaml` - Engine Pod Disruption Budget

For production, we recommend using a managed PostgreSQL service (e.g. Amazon RDS, Cloud SQL, Azure Database) instead of the bundled single-replica StatefulSet.

## Local Development

For local development with k3d:

```bash
./scripts/run/k8s/engine.sh
```

This script creates a k3d cluster, builds the image, and deploys everything.

## Production Deployment

For production deployments, see the steps outlined in our [Kubernetes Self-Hosting Guide](https://rivet.dev/actors/self-host/control-plane/kubernetes).
