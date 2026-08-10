#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

docker build -t registry.example.com/your-team/rivetkit-app:latest .
docker push registry.example.com/your-team/rivetkit-app:latest

kubectl apply -f rivetkit-secrets.yaml
kubectl apply -f deployment.yaml

kubectl get pods -l app=rivetkit-app
