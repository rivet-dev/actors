#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

gcloud builds submit --tag us-central1-docker.pkg.dev/YOUR_PROJECT/rivetkit-app/rivetkit-app:latest

gcloud run deploy rivetkit-app \
  --image us-central1-docker.pkg.dev/YOUR_PROJECT/rivetkit-app/rivetkit-app:latest \
  --region us-central1 \
  --allow-unauthenticated \
  --min-instances 1 \
  --set-env-vars RIVET_ENDPOINT=<your-rivet-endpoint>,RIVET_PUBLIC_ENDPOINT=<your-rivet-public-endpoint>,RIVETKIT_RUNTIME_MODE=serverless

gcloud run services describe rivetkit-app --region us-central1 --format 'value(status.conditions[?type="Ready"].status)'
