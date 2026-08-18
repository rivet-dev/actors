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
	cd scripts/skill-evals && npx tsx src/index.ts --eval {{name}} {{args}}

[group('skill-evals')]
skill-eval-clean:
	rm -rf scripts/skill-evals/results
