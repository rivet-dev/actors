import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import * as fs from 'node:fs';
import { createRequire } from 'node:module';
import { createServer } from 'node:net';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createClient } from 'rivetkit/client';

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const fixtureRoots = [];
const require = createRequire(import.meta.url);
const flueCliRoot = path.resolve(
	path.dirname(fileURLToPath(import.meta.resolve('@flue/cli'))),
	'..',
);
const flueRuntimeRoot = path.resolve(
	path.dirname(fileURLToPath(import.meta.resolve('@flue/runtime'))),
	'..',
);
const cli = pathToFileURL(path.join(flueCliRoot, 'bin', 'flue.mjs'));
const { getEnginePath } = require('@rivetkit/engine-cli');

process.on('exit', () => {
	for (const root of fixtureRoots) fs.rmSync(root, { recursive: true, force: true });
});

test('builds and serves an agent through target: rivet', async () => {
	const root = createFixtureRoot();
	const port = await getAvailablePort();
	const enginePort = await getAvailablePort();
	const enginePeerPort = await getAvailablePort();
	const engineMetricsPort = await getAvailablePort();
	const runKey = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
	const poolName = `flue-${Date.now()}-${Math.random().toString(36).slice(2)}`;
	const registryKey = `registry-${Date.now()}-${Math.random().toString(36).slice(2)}`;
	writeProject(root);

	const build = await runCli(root, ['build']);
	assert.equal(build.code, 0, build.stderr);
	assert.equal(fs.existsSync(path.join(root, 'dist', 'server.mjs')), true);

	const syntax = spawnSync(process.execPath, ['--check', path.join(root, 'dist', 'server.mjs')], {
		encoding: 'utf8',
	});
	assert.equal(syntax.status, 0, syntax.stderr);

	const engine = startEngine({
		guardPort: enginePort,
		peerPort: enginePeerPort,
		metricsPort: engineMetricsPort,
	});
	let dev: ReturnType<typeof startDev> | undefined;
	let passed = false;
	try {
		await waitForMetadata(enginePort, engine.logs);
		await configureNormalRunnerConfig(poolName, enginePort);
		dev = startDev(root, port, {
			RIVET_RUN_ENGINE: '0',
			RIVET_ENDPOINT: `http://127.0.0.1:${enginePort}`,
			RIVET_POOL: poolName,
			FLUE_RIVET_REGISTRY_KEY: registryKey,
		});
		await waitForServer(port, dev.logs);
			const client = createClient<any>({ endpoint: `http://127.0.0.1:${enginePort}`, poolName });
			try {
				await client.health.getOrCreate(['test']).ping('ready');
			} catch (error) {
			assert.fail(`${String(error)}\n\n${dev.logs()}`);
		}
		await client.dispose();
		const directInstanceId = (attempt) => `instance-${runKey}-${attempt}`;
		const prompt = await postJsonWithRetry(
			(attempt) => `http://127.0.0.1:${port}/agents/assistant/${directInstanceId(attempt)}`,
			() => ({ kind: 'user', body: 'Hello Rivet' }),
			dev.logs,
			202,
		);
		assert.equal(typeof prompt.json.submissionId, 'string');

		const stream = await fetchConversationWithRetry(
			`http://127.0.0.1:${port}/agents/assistant/${directInstanceId(prompt.attempt)}?view=history`,
			dev.logs,
		);
		assert.ok(stream.messages.some((message) =>
			message.role === 'assistant' &&
			message.parts.some((part) => part.type === 'text' && part.text === 'Hello from target rivet.'),
		), JSON.stringify(stream));

		const workflow = await postJsonWithRetry(
			() => `http://127.0.0.1:${port}/workflows/job?wait=result`,
			() => undefined,
			dev.logs,
		);
		const workflowBody = workflow.json;
		assert.equal(typeof workflowBody.result.dispatchId, 'string');
		const runMeta = await fetchOkTextWithRetry(
			`http://127.0.0.1:${port}/runs/${workflowBody.runId}?meta`,
			dev.logs,
		);
		assert.equal(JSON.parse(runMeta.text).status, 'completed');

		const runStream = await fetchOkTextWithRetry(
			`http://127.0.0.1:${port}/runs/${workflowBody.runId}`,
			dev.logs,
		);
		assert.match(runStream.text, /run_start|run_end/);
		passed = true;

	} finally {
		await dev?.stop();
		await engine.stop();
		killEngineOnPort(enginePort);
		if (passed) setTimeout(() => process.exit(0), 100);
	}
});

function createFixtureRoot() {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'flue-rivet-target-'));
	fixtureRoots.push(root);
	fs.mkdirSync(path.join(root, 'node_modules', '@flue'), { recursive: true });
	fs.symlinkSync(
		flueRuntimeRoot,
		path.join(root, 'node_modules', '@flue', 'runtime'),
		'dir',
	);
	fs.symlinkSync(
		flueCliRoot,
		path.join(root, 'node_modules', '@flue', 'cli'),
		'dir',
	);
	fs.mkdirSync(path.join(root, 'node_modules', '@rivet-dev'), { recursive: true });
	fs.symlinkSync(packageRoot, path.join(root, 'node_modules', '@rivet-dev', 'flue'), 'dir');
	fs.symlinkSync(path.join(packageRoot, 'node_modules', 'rivetkit'), path.join(root, 'node_modules', 'rivetkit'), 'dir');
	return root;
}

function writeProject(root) {
	fs.writeFileSync(
		path.join(root, 'package.json'),
		JSON.stringify({
			type: 'module',
			dependencies: {
				'@flue/cli': `link:${flueCliRoot}`,
				'@flue/runtime': `link:${flueRuntimeRoot}`,
				'@rivet-dev/flue': `link:${packageRoot}`,
				rivetkit: `link:${path.join(packageRoot, 'node_modules', 'rivetkit')}`,
			},
		}),
	);
	fs.writeFileSync(
		path.join(root, 'flue.config.ts'),
		`import { defineConfig } from '@flue/cli/config';\nimport { rivet } from '@rivet-dev/flue';\nexport default defineConfig({ target: rivet({ actors: './actors.ts' }) });\n`,
	);
	fs.writeFileSync(
		path.join(root, 'actors.ts'),
		`import { actor, setup } from 'rivetkit';\nconst health = actor({ actions: { ping: (_c, value) => value } });\nexport const registry = setup({ use: { health } });\n`,
	);
	fs.mkdirSync(path.join(root, 'agents'), { recursive: true });
	fs.writeFileSync(
		path.join(root, 'agents', 'assistant.ts'),
		`import { createAgent, registerProvider } from '@flue/runtime';\nimport { fauxAssistantMessage, registerFauxProvider } from '@flue/runtime/adapter-kit';\nexport const route = async (_c, next) => next();\nconst provider = registerFauxProvider({ provider: 'rivet-target-' + crypto.randomUUID() });\nprovider.setResponses([fauxAssistantMessage('Hello from target rivet.'), fauxAssistantMessage('Dispatched from workflow.')]);\nconst model = provider.getModel();\nregisterProvider(model.provider, { api: provider.api, baseUrl: model.baseUrl });\nexport default createAgent(() => ({ model: model.provider + '/' + model.id }));\n`,
	);
	fs.mkdirSync(path.join(root, 'workflows'), { recursive: true });
	fs.writeFileSync(
		path.join(root, 'workflows', 'job.ts'),
		`import { defineWorkflow, dispatch } from '@flue/runtime';\nimport agent from '../agents/assistant.ts';\nexport const route = async (_c, next) => next();\nexport const runs = async (_c, next) => next();\nexport default defineWorkflow({ agent, async run() {\n  const receipt = await dispatch({ agent: 'assistant', id: 'workflow-dispatched', message: { kind: 'user', body: 'Dispatch from workflow' } });\n  return { dispatchId: receipt.dispatchId };\n} });\n`,
	);
}

async function runCli(cwd, args) {
	const child = spawn(process.execPath, [cli.pathname, ...args], {
		cwd,
		stdio: ['ignore', 'pipe', 'pipe'],
	});
	let stdout = '';
	let stderr = '';
	child.stdout.setEncoding('utf8');
	child.stderr.setEncoding('utf8');
	child.stdout.on('data', (chunk) => {
		stdout += chunk;
	});
	child.stderr.on('data', (chunk) => {
		stderr += chunk;
	});
	const [code, signal] = await once(child, 'exit');
	return { code, signal, stdout, stderr };
}

function startDev(cwd, port, env = {}) {
	const child = spawn(process.execPath, [cli.pathname, 'dev', '--port', String(port)], {
		cwd,
		env: {
			...process.env,
			...env,
			RIVET_RUN_ENGINE: env.RIVET_RUN_ENGINE ?? '1',
		},
		stdio: ['ignore', 'pipe', 'pipe'],
	});
	let output = '';
	for (const stream of [child.stdout, child.stderr]) {
		stream.setEncoding('utf8');
		stream.on('data', (chunk) => {
			output += chunk;
		});
	}
	return {
		logs() {
			return output;
		},
		async stop() {
			if (child.exitCode !== null || child.signalCode !== null) return;
			child.kill('SIGTERM');
			await Promise.race([
				once(child, 'exit'),
				new Promise((_, reject) =>
					setTimeout(() => reject(new Error(`Timed out stopping flue dev\n\n${output}`)), 5_000),
				),
			]);
		},
	};
}

function startEngine({ guardPort, peerPort, metricsPort }) {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), 'flue-rivet-engine-'));
	const configPath = path.join(root, 'config.json');
	fs.writeFileSync(
		configPath,
		JSON.stringify({
			api_peer: { port: peerPort },
			file_system: { path: path.join(root, 'db') },
			guard: { port: guardPort },
			metrics: { port: metricsPort },
			telemetry: { enabled: false },
			topology: {
				datacenter_label: 1,
				datacenters: {
					default: {
						datacenter_label: 1,
						is_leader: true,
						peer_url: `http://127.0.0.1:${peerPort}`,
						public_url: `http://127.0.0.1:${guardPort}`,
					},
				},
			},
		}),
	);
	const child = spawn(getEnginePath(), ['start', '--config', configPath], {
		stdio: ['ignore', 'pipe', 'pipe'],
	});
	let output = '';
	for (const stream of [child.stdout, child.stderr]) {
		stream.setEncoding('utf8');
		stream.on('data', (chunk) => {
			output += chunk;
		});
	}
	return {
		logs() {
			return output;
		},
		async stop() {
			if (child.exitCode === null && child.signalCode === null) {
				child.kill('SIGTERM');
				await Promise.race([
					once(child, 'exit'),
					new Promise((_, reject) =>
						setTimeout(() => reject(new Error(`Timed out stopping rivet-engine\n\n${output}`)), 5_000),
					),
				]).catch(() => child.kill('SIGKILL'));
			}
			fs.rmSync(root, { recursive: true, force: true });
		},
	};
}

async function getAvailablePort() {
	const server = createServer();
	server.listen(0, '127.0.0.1');
	await once(server, 'listening');
	const address = server.address();
	assert(address && typeof address === 'object');
	server.close();
	await once(server, 'close');
	return address.port;
}

async function waitForServer(port, logs = () => '') {
	await waitFor(
		async () => {
			try {
				const response = await fetch(`http://127.0.0.1:${port}/`);
				return response.status < 500;
			} catch {
				return false;
			}
		},
		() => `Timed out waiting for server on port ${port}\n\n${logs()}`,
	);
}

async function waitForMetadata(port, logs = () => '', timeout = 45_000) {
	const deadline = Date.now() + timeout;
	while (Date.now() < deadline) {
		try {
			const response = await fetch(`http://127.0.0.1:${port}/metadata`);
			if (response.ok) return;
		} catch {}
		await new Promise((resolve) => setTimeout(resolve, 250));
	}
	throw new Error(`Timed out waiting for Rivet metadata on port ${port}\n\n${logs()}`);
}

async function configureNormalRunnerConfig(poolName, port) {
	const datacenters = await fetch(`http://127.0.0.1:${port}/datacenters`);
	const datacentersText = await datacenters.text();
	assert.equal(datacenters.status, 200, datacentersText);
	const body = JSON.parse(datacentersText);
	const response = await fetch(`http://127.0.0.1:${port}/runner-configs/${poolName}?namespace=default`, {
		method: 'PUT',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({
			datacenters: Object.fromEntries(
				body.datacenters.map((dc) => [
					dc.name,
					{
						normal: {
							drain_on_version_upgrade: false,
						},
						metadata: {},
					},
				]),
			),
		}),
	});
	assert.equal(response.status, 200, await response.text());
}

async function waitFor(predicate, message, timeout = 20_000) {
	const deadline = Date.now() + timeout;
	while (Date.now() < deadline) {
		if (await predicate()) return;
		await new Promise((resolve) => setTimeout(resolve, 50));
	}
	throw new Error(typeof message === 'function' ? message() : message);
}

async function postJsonWithRetry(urlForAttempt, bodyForAttempt, logs = () => '', expectedStatus = 200) {
	const result = await fetchOkTextWithRetry(
		urlForAttempt,
		logs,
		(attempt) => ({
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(bodyForAttempt(attempt)),
		}),
		expectedStatus,
	);
	return { json: JSON.parse(result.text), attempt: result.attempt };
}

async function fetchOkTextWithRetry(
	urlOrFactory,
	logs = () => '',
	initForAttempt = () => undefined,
	expectedStatus = 200,
) {
	let lastText = '';
	for (let attempt = 0; attempt < 5; attempt++) {
		const url = typeof urlOrFactory === 'function' ? urlOrFactory(attempt) : urlOrFactory;
		let response;
		try {
			response = await fetchWithTimeout(url, initForAttempt(attempt), 30_000);
			lastText = await response.text();
		} catch (error) {
			lastText = error instanceof Error ? error.stack || error.message : String(error);
		}
		if (response?.status === expectedStatus) return { text: lastText, attempt };
		const diagnostics = `${lastText}\n\n${logs()}`;
		if (response && response.status < 500 && !/actor|envoy|ready|wake|fetch failed|aborted|timeout/i.test(diagnostics)) {
			assert.equal(response.status, expectedStatus, diagnostics);
		}
		await new Promise((resolve) => setTimeout(resolve, 2_000 * (attempt + 1)));
	}
	assert.fail(`${lastText}\n\n${logs()}`);
}

async function fetchConversationWithRetry(url, logs = () => '') {
	let lastText = '';
	for (let attempt = 0; attempt < 20; attempt++) {
		const response = await fetchWithTimeout(url, undefined, 30_000);
		lastText = await response.text();
		if (response.ok) {
			const snapshot = JSON.parse(lastText);
			if (snapshot.messages?.some((message) =>
				message.role === 'assistant' &&
				message.parts?.some((part) => part.type === 'text' && part.state === 'done'),
			)) return snapshot;
		}
		await new Promise((resolve) => setTimeout(resolve, 500));
	}
	assert.fail(`${lastText}\n\n${logs()}`);
}

function fetchWithTimeout(url, init, timeout) {
	return fetch(url, { ...init, signal: AbortSignal.timeout(timeout) });
}

function killEngineOnPort(port) {
	try {
		const output = execFileSync('ss', ['-ltnp'], { encoding: 'utf8' });
		const portPattern = new RegExp(`:${port}\\b`);
		for (const line of output.split('\n')) {
			if (!portPattern.test(line)) continue;
			const match = line.match(/pid=(\d+)/);
			if (match) process.kill(Number(match[1]), 'SIGKILL');
		}
	} catch {}
}
