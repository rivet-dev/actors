[group('release')]
release *ARGS:
	pnpm --filter=publish release {{ ARGS }}

[group('release')]
preview-publish REF:
	gh workflow run .github/workflows/publish.yaml --ref "{{ REF }}"

[group('docker')]
docker-build:
	docker build -f engine/docker/universal/Dockerfile --target engine-full -t rivetdev/engine:local --platform linux/x86_64 .

[group('docker')]
docker-build-frontend:
	docker build -f engine/docker/universal/Dockerfile --target engine-full -t rivetdev/engine:local --platform linux/x86_64 --build-arg BUILD_FRONTEND=true .

[group('docker')]
docker-run:
	docker run -p 6420:6420 -e RIVET__AUTH__ADMIN_TOKEN=dev -e RUST_LOG=debug rivetdev/engine:local

[group('docker')]
docker-stop:
	docker run -p 6420:6420 -e RIVET__AUTH__ADMIN_TOKEN=dev -e RUST_LOG=debug rivetdev/engine:local

[group('skill-evals')]
skill-eval name *args:
	cd scripts/skill-evals && ./node_modules/.bin/tsx src/index.ts --eval {{name}} {{args}}

[group('skill-evals')]
skill-evals *args:
	cd scripts/skill-evals && ./node_modules/.bin/tsx src/index.ts {{args}}

[group('skill-evals')]
skill-eval-clean:
	rm -rf scripts/skill-evals/results

# --- rivet.dev website (landing + /docs) ---
# The focused install keeps website development independent from unrelated
# workspace packages. Building the icon set needs FONTAWESOME_PACKAGE_TOKEN
# (for example, `source ~/misc/env.txt` first).

[group('website')]
dev-website-setup:
	#!/usr/bin/env bash
	set -euo pipefail
	icons="frontend/packages/icons"
	built=0

	# Install the website and its workspace dependencies without rewriting the
	# repository lockfile. This is local development setup, not a release install.
	pnpm --filter 'rivet-website...' install --lockfile=false

	# Generate the private icon package when this workspace has no local build.
	if [ ! -f "$icons/dist/index.js" ]; then
		if [ -z "${FONTAWESOME_PACKAGE_TOKEN:-}" ]; then
			echo "error: FONTAWESOME_PACKAGE_TOKEN is required to build @rivet-gg/icons." >&2
			echo "       export it (e.g. 'source ~/misc/env.txt') and re-run." >&2
			exit 1
		fi
		pnpm --filter @rivet-gg/icons generate
		built=1
	fi

	# file: and workspace dependencies may copy generated output into the store.
	if [ "$built" = 1 ]; then
		pnpm --filter 'rivet-website...' install --lockfile=false
	fi

	# Refresh only the site package last so Astro and the generated icons resolve
	# immediately without installing unrelated workspace examples.
	pnpm --filter rivet-website install --lockfile=false

	echo "dev-website-setup: ready"

[group('website')]
dev-website-start:
	pnpm --filter rivet-website dev

[group('website')]
dev-website: dev-website-setup dev-website-start

[group('website')]
dev-website-build: dev-website-setup
	pnpm --filter rivet-website build
