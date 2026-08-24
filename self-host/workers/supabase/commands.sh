#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

npx supabase secrets set \
  RIVET_ENDPOINT=https://your-namespace:sk_...@api.rivet.dev \
  RIVET_PUBLIC_ENDPOINT=https://your-namespace@api.rivet.dev

npx supabase functions deploy rivet
