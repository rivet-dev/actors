// Minimal WebSocket + HTTP "game server" stand-in.
//
// This is the child process that container-runner (rivet-container-runner) spawns inside
// the container. It stands in for a real Unity FishNet dedicated server while we validate
// the Rivet -> serverless -> container pipeline.
//
// Normal behavior:
//   - Binds HTTP+WebSocket on $PORT (default 7770) on 0.0.0.0.
//   - GET /            -> 200 "ok"
//   - GET /health      -> 200 "healthy"
//   - GET|POST /reflect-> 200 JSON echoing method/path/headers/body (for HTTP proxy tests)
//   - WebSocket        -> sends "welcome", then echoes every message as "echo: <msg>"
//
// Failure-injection modes (via env, so tests can drive them through the actor `input.env`):
//   CRASH_ON_START=1        exit(1) immediately, before listening (start-failure path)
//   HANG_NO_LISTEN=1        run forever but never open the port (readiness-timeout path)
//   CRASH_AFTER_MS=<n>      become ready, then exit(1) after n ms (mid-session crash)
//   EXIT_CODE=<n>           exit code to use for the crash modes (default 1)
//
// It also logs argv + a couple of env vars at startup so tests can verify that
// input.command / input.args / input.env were applied.

import http from "node:http";
import { WebSocketServer } from "ws";

const PORT = Number.parseInt(process.env.PORT ?? "7770", 10);
const HOST = process.env.HOST ?? "0.0.0.0";
const EXIT_CODE = Number.parseInt(process.env.EXIT_CODE ?? "1", 10);

function log(...args) {
  console.log("[test-server]", ...args);
}

// Surface how we were launched (so input override tests can assert on this).
log(`argv=${JSON.stringify(process.argv.slice(2))}`);
log(`env CUSTOM_ENV=${process.env.CUSTOM_ENV ?? "<unset>"} GREETING=${process.env.GREETING ?? "<unset>"}`);

// ---- failure injection: crash before ready ----
if (process.env.CRASH_ON_START === "1") {
  log(`CRASH_ON_START set, exiting ${EXIT_CODE} before listening`);
  process.exit(EXIT_CODE);
}

// ---- failure injection: run forever but never listen ----
if (process.env.HANG_NO_LISTEN === "1") {
  log("HANG_NO_LISTEN set, running without ever opening the port");
  setInterval(() => {}, 1 << 30);
} else {
  startServer();
}

function startServer() {
  const server = http.createServer((req, res) => {
    if (req.url === "/health") {
      res.writeHead(200, { "content-type": "text/plain" });
      res.end("healthy");
      return;
    }
    if (req.url?.startsWith("/reflect")) {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        const body = Buffer.concat(chunks).toString();
        log(`http ${req.method} ${req.url} body=${JSON.stringify(body)}`);
        res.writeHead(200, { "content-type": "application/json" });
        res.end(
          JSON.stringify({
            method: req.method,
            path: req.url,
            headers: req.headers,
            body,
          })
        );
      });
      return;
    }
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("ok");
  });

  const wss = new WebSocketServer({ server });

  wss.on("connection", (ws, req) => {
    log(`ws connection opened from ${req.socket.remoteAddress} (path=${req.url})`);
    ws.on("message", (data, isBinary) => {
      const text = isBinary ? `<binary ${data.length} bytes>` : data.toString();
      log(`ws message received: ${text}`);
      if (isBinary) ws.send(data, { binary: true });
      else ws.send(`echo: ${text}`);
    });
    ws.on("close", (code, reason) => log(`ws connection closed (code=${code} reason=${reason})`));
    ws.on("error", (err) => log(`ws error: ${err?.message ?? err}`));
    ws.send("welcome");
  });

  server.listen(PORT, HOST, () => {
    log(`listening on http://${HOST}:${PORT} (pid=${process.pid})`);

    // ---- failure injection: crash after becoming ready ----
    const crashAfter = Number.parseInt(process.env.CRASH_AFTER_MS ?? "", 10);
    if (Number.isFinite(crashAfter)) {
      log(`CRASH_AFTER_MS=${crashAfter}, will exit ${EXIT_CODE} after that`);
      setTimeout(() => {
        log(`CRASH_AFTER_MS elapsed, exiting ${EXIT_CODE}`);
        process.exit(EXIT_CODE);
      }, crashAfter).unref();
    }
  });

  for (const sig of ["SIGTERM", "SIGINT"]) {
    process.on(sig, () => {
      log(`received ${sig}, shutting down`);
      wss.close();
      server.close(() => {
        log("server closed, exiting");
        process.exit(0);
      });
      setTimeout(() => process.exit(0), 3000).unref();
    });
  }
}
