// Configure a SERVERLESS runner pool that points the engine at the game container.
// PUT /runner-configs/{runner_name}?namespace=default   (bearer auth)
import { ENGINE, NAMESPACE, TOKEN, DATACENTER, RUNNER_NAME, SERVERLESS_URL } from "./common.mjs";

const body = {
  datacenters: {
    [DATACENTER]: {
      serverless: {
        url: SERVERLESS_URL,
        headers: {},
        // Seconds the engine holds the /start request before draining to a fresh one.
        request_lifespan: 900,
        drain_grace_period: 30,
        // 1:1 actor<->container mapping (serverless concurrency=1 model).
        slots_per_runner: 1,
        max_runners: 1,
        max_concurrent_actors: 1,
      },
    },
  },
};

const url = `${ENGINE}/runner-configs/${RUNNER_NAME}?namespace=${encodeURIComponent(NAMESPACE)}`;
console.log(`PUT ${url}`);
console.log(`body: ${JSON.stringify(body)}`);

const res = await fetch(url, {
  method: "PUT",
  headers: { "content-type": "application/json", authorization: `Bearer ${TOKEN}` },
  body: JSON.stringify(body),
});

const text = await res.text();
console.log(`status: ${res.status}`);
console.log(`response: ${text}`);
if (!res.ok) process.exit(1);
