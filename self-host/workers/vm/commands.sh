#!/usr/bin/env bash
# Commands from the guide, in order.
set -euo pipefail

scp -r ./dist user@server:/opt/rivetkit-app

sudo systemctl daemon-reload
sudo systemctl enable --now rivetkit-app.service

sudo systemctl restart rivetkit-app.service

sudo journalctl -u rivetkit-app.service -f
