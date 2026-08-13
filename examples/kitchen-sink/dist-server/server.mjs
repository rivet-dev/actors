var __defProp = Object.defineProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// src/server.ts
import { existsSync, readFileSync } from "fs";
import { resolve } from "path";
import * as v8 from "v8";
import { serve } from "@hono/node-server";
import { serveStatic } from "@hono/node-server/serve-static";
import { Hono as Hono3 } from "hono";

// src/index.ts
import { setup as setup2 } from "rivetkit";

// src/actors/actions/action-inputs.ts
import { actor } from "rivetkit";
var inputActor = actor({
  createState: (_c, input) => {
    return {
      initialInput: input,
      onCreateInput: void 0
    };
  },
  onCreate: (c, input) => {
    c.state.onCreateInput = input;
  },
  actions: {
    getInputs: (c) => {
      return {
        initialInput: c.state.initialInput,
        onCreateInput: c.state.onCreateInput
      };
    }
  }
});

// src/actors/actions/action-timeout.ts
import { actor as actor2 } from "rivetkit";
var shortTimeoutActor = actor2({
  state: { value: 0 },
  options: {
    actionTimeout: 50
    // 50ms timeout
  },
  actions: {
    quickAction: async (_c) => {
      return "quick response";
    },
    slowAction: async (_c) => {
      await new Promise((resolve2) => setTimeout(resolve2, 100));
      return "slow response";
    }
  }
});
var longTimeoutActor = actor2({
  state: { value: 0 },
  options: {
    actionTimeout: 200
    // 200ms timeout
  },
  actions: {
    delayedAction: async (_c) => {
      await new Promise((resolve2) => setTimeout(resolve2, 100));
      return "delayed response";
    }
  }
});
var defaultTimeoutActor = actor2({
  state: { value: 0 },
  actions: {
    normalAction: async (_c) => {
      await new Promise((resolve2) => setTimeout(resolve2, 50));
      return "normal response";
    }
  }
});
var syncTimeoutActor = actor2({
  state: { value: 0 },
  options: {
    actionTimeout: 50
    // 50ms timeout
  },
  actions: {
    syncAction: (_c) => {
      return "sync response";
    }
  }
});

// src/actors/actions/action-types.ts
import { actor as actor3, UserError } from "rivetkit";
var syncActionActor = actor3({
  state: { value: 0 },
  actions: {
    // Simple synchronous action that returns a value directly
    increment: (c, amount = 1) => {
      c.state.value += amount;
      return c.state.value;
    },
    // Synchronous action that returns an object
    getInfo: (c) => {
      return {
        currentValue: c.state.value,
        timestamp: Date.now()
      };
    },
    // Synchronous action with no return value (void)
    reset: (c) => {
      c.state.value = 0;
    }
  }
});
var asyncActionActor = actor3({
  state: { value: 0, data: null },
  actions: {
    // Async action with a delay
    delayedIncrement: async (c, amount = 1) => {
      await Promise.resolve();
      c.state.value += amount;
      return c.state.value;
    },
    // Async action that simulates an API call
    fetchData: async (c, id) => {
      await Promise.resolve();
      const data = { id, timestamp: Date.now() };
      c.state.data = data;
      return data;
    },
    // Async action with error handling
    asyncWithError: async (_c, shouldError) => {
      await Promise.resolve();
      if (shouldError) {
        throw new UserError("Intentional error");
      }
      return "Success";
    }
  }
});
var promiseActor = actor3({
  state: { results: [] },
  actions: {
    // Action that returns a resolved promise
    resolvedPromise: (_c) => {
      return Promise.resolve("resolved value");
    },
    // Action that returns a promise that resolves after a delay
    delayedPromise: (c) => {
      return new Promise((resolve2) => {
        c.state.results.push("delayed");
        resolve2("delayed value");
      });
    },
    // Action that returns a rejected promise
    rejectedPromise: (_c) => {
      return Promise.reject(new UserError("promised rejection"));
    },
    // Action to check the collected results
    getResults: (c) => {
      return c.state.results;
    }
  }
});

// src/actors/actions/error-handling.ts
import { actor as actor4, UserError as UserError2 } from "rivetkit";
var errorHandlingActor = actor4({
  state: {
    errorLog: []
  },
  actions: {
    // Action that throws a UserError with just a message
    throwSimpleError: () => {
      throw new UserError2("Simple error message");
    },
    // Action that throws a UserError with code and metadata
    throwDetailedError: () => {
      throw new UserError2("Detailed error message", {
        code: "detailed_error",
        metadata: {
          reason: "test",
          timestamp: Date.now()
        }
      });
    },
    // Action that throws an internal error
    throwInternalError: () => {
      throw new Error("This is an internal error");
    },
    // Action that returns successfully
    successfulAction: () => {
      return "success";
    },
    // Action that times out (simulated with a long delay)
    timeoutAction: async (_c) => {
      return new Promise((resolve2) => {
        setTimeout(() => {
          resolve2("This should not be reached if timeout works");
        }, 1e4);
      });
    },
    // Action with configurable delay to test timeout edge cases
    delayedAction: async (_c, delayMs) => {
      return new Promise((resolve2) => {
        setTimeout(() => {
          resolve2(`Completed after ${delayMs}ms`);
        }, delayMs);
      });
    },
    // Log an error for inspection
    logError: (c, error) => {
      c.state.errorLog.push(error);
      return c.state.errorLog;
    },
    // Get the error log
    getErrorLog: (c) => {
      return c.state.errorLog;
    },
    // Clear the error log
    clearErrorLog: (c) => {
      c.state.errorLog = [];
      return true;
    }
  },
  options: {
    actionTimeout: 500
    // 500ms timeout for actions
  }
});
var customTimeoutActor = actor4({
  state: {},
  actions: {
    quickAction: async () => {
      await new Promise((resolve2) => setTimeout(resolve2, 50));
      return "Quick action completed";
    },
    slowAction: async () => {
      await new Promise((resolve2) => setTimeout(resolve2, 300));
      return "Slow action completed";
    }
  },
  options: {
    actionTimeout: 200
    // 200ms timeout
  }
});

// src/actors/ai/ai-agent.ts
import { openai } from "@ai-sdk/openai";
import { generateText, tool } from "ai";
import { actor as actor5, event } from "rivetkit";
import { z } from "zod";

// src/actors/ai/my-tools.ts
async function getWeather(location) {
  return {
    location,
    temperature: Math.floor(Math.random() * 30) + 10,
    condition: ["sunny", "cloudy", "rainy", "snowy"][Math.floor(Math.random() * 4)],
    humidity: Math.floor(Math.random() * 50) + 30
  };
}

// src/actors/ai/ai-agent.ts
var aiAgent = actor5({
  // Persistent state that survives restarts: https://rivet.dev/docs/actors/state
  state: {
    messages: []
  },
  events: {
    messageReceived: event()
  },
  actions: {
    // Callable functions from clients: https://rivet.dev/docs/actors/actions
    getMessages: (c) => c.state.messages,
    sendMessage: async (c, userMessage) => {
      const userMsg = {
        role: "user",
        content: userMessage,
        timestamp: Date.now()
      };
      c.state.messages.push(userMsg);
      const { text: text2 } = await generateText({
        model: openai("gpt-4o-mini"),
        prompt: userMessage,
        messages: c.state.messages,
        tools: {
          weather: tool({
            description: "Get the weather in a location",
            parameters: z.object({
              location: z.string().describe(
                "The location to get the weather for"
              )
            }),
            execute: async ({ location }) => {
              return await getWeather(location);
            }
          })
        }
      });
      const assistantMsg = {
        role: "assistant",
        content: text2,
        timestamp: Date.now()
      };
      c.state.messages.push(assistantMsg);
      c.broadcast("messageReceived", assistantMsg);
      return assistantMsg;
    }
  }
});

// src/actors/connections/conn-state.ts
import { actor as actor6, event as event2 } from "rivetkit";
var connStateActor = actor6({
  state: {
    sharedCounter: 0,
    disconnectionCount: 0
  },
  events: {
    userConnected: event2(),
    userDisconnected: event2(),
    directMessage: event2()
  },
  // Define connection state
  createConnState: (_c, params) => {
    return {
      username: params?.username || "anonymous",
      role: params?.role || "user",
      counter: 0,
      createdAt: Date.now(),
      noCount: params?.noCount ?? false
    };
  },
  // Lifecycle hook when a connection is established
  onConnect: (c, conn) => {
    c.broadcast("userConnected", {
      id: conn.id,
      username: "anonymous",
      role: "user"
    });
  },
  // Lifecycle hook when a connection is closed
  onDisconnect: (c, conn) => {
    if (!conn.state?.noCount) {
      c.state.disconnectionCount += 1;
      c.broadcast("userDisconnected", {
        id: conn.id
      });
    }
  },
  actions: {
    // Action to increment the connection's counter
    incrementConnCounter: (c, amount = 1) => {
      c.conn.state.counter += amount;
    },
    // Action to increment the shared counter
    incrementSharedCounter: (c, amount = 1) => {
      c.state.sharedCounter += amount;
      return c.state.sharedCounter;
    },
    // Get the connection state
    getConnectionState: (c) => {
      return { id: c.conn.id, ...c.conn.state };
    },
    // Check all active connections
    getConnectionIds: (c) => {
      return c.conns.entries().filter((c2) => !c2[1].state?.noCount).map((x) => x[0]).toArray();
    },
    // Get disconnection count
    getDisconnectionCount: (c) => {
      return c.state.disconnectionCount;
    },
    // Get all active connection states
    getAllConnectionStates: (c) => {
      return c.conns.entries().map(([id, conn]) => ({ id, ...conn.state })).toArray();
    },
    // Send message to a specific connection with matching ID
    sendToConnection: (c, targetId, message) => {
      if (c.conns.has(targetId)) {
        c.conns.get(targetId)?.send("directMessage", { from: c.conn.id, message });
        return true;
      } else {
        return false;
      }
    },
    // Update connection state (simulated for tests)
    updateConnection: (c, updates) => {
      if (updates.username) c.conn.state.username = updates.username;
      if (updates.role) c.conn.state.role = updates.role;
      return c.conn.state;
    },
    disconnectSelf: (c, reason) => {
      c.conn.disconnect(reason ?? "test.disconnect");
      return true;
    }
  }
});

// src/actors/connections/reject-connection.ts
import { actor as actor7, UserError as UserError3 } from "rivetkit";
var rejectConnectionActor = actor7({
  onBeforeConnect: async (_c, params) => {
    if (params?.reject) {
      await new Promise((resolve2) => setTimeout(resolve2, 500));
      throw new UserError3("Rejected connection", {
        code: "rejected"
      });
    }
  },
  actions: {
    ping: () => "pong"
  }
});

// src/actors/connections/request-access.ts
import { actor as actor8 } from "rivetkit";
var requestAccessActor = actor8({
  state: {
    // Track request info from different hooks
    onBeforeConnectRequest: {
      hasRequest: false,
      requestUrl: null,
      requestMethod: null,
      requestHeaders: {}
    },
    createConnStateRequest: {
      hasRequest: false,
      requestUrl: null,
      requestMethod: null,
      requestHeaders: {}
    },
    onRequestRequest: {
      hasRequest: false,
      requestUrl: null,
      requestMethod: null,
      requestHeaders: {}
    },
    onWebSocketRequest: {
      hasRequest: false,
      requestUrl: null,
      requestMethod: null,
      requestHeaders: {}
    }
  },
  createConnState: (c, params) => {
    let requestInfo = null;
    if (params?.trackRequest && c.request) {
      const headers = {};
      c.request.headers.forEach((value, key) => {
        headers[key] = value;
      });
      requestInfo = {
        hasRequest: true,
        requestUrl: c.request.url,
        requestMethod: c.request.method,
        requestHeaders: headers
      };
    }
    return {
      trackRequest: params?.trackRequest || false,
      requestInfo
    };
  },
  onConnect: (c, conn) => {
    if (conn.state.requestInfo) {
      c.state.createConnStateRequest = conn.state.requestInfo;
    }
  },
  onBeforeConnect: (c, params) => {
    if (params?.trackRequest) {
      if (c.request) {
        c.state.onBeforeConnectRequest.hasRequest = true;
        c.state.onBeforeConnectRequest.requestUrl = c.request.url;
        c.state.onBeforeConnectRequest.requestMethod = c.request.method;
        const headers = {};
        c.request.headers.forEach((value, key) => {
          headers[key] = value;
        });
        c.state.onBeforeConnectRequest.requestHeaders = headers;
      } else {
        c.state.onBeforeConnectRequest.hasRequest = false;
      }
    }
  },
  onRequest: (c, request) => {
    c.state.onRequestRequest.hasRequest = true;
    c.state.onRequestRequest.requestUrl = request.url;
    c.state.onRequestRequest.requestMethod = request.method;
    const headers = {};
    request.headers.forEach((value, key) => {
      headers[key] = value;
    });
    c.state.onRequestRequest.requestHeaders = headers;
    return new Response(
      JSON.stringify({
        hasRequest: true,
        requestUrl: request.url,
        requestMethod: request.method,
        requestHeaders: headers
      }),
      {
        status: 200,
        headers: { "Content-Type": "application/json" }
      }
    );
  },
  onWebSocket: (c, websocket) => {
    if (!c.request) throw "Missing request";
    c.state.onWebSocketRequest.hasRequest = true;
    c.state.onWebSocketRequest.requestUrl = c.request.url;
    c.state.onWebSocketRequest.requestMethod = c.request.method;
    const headers = {};
    c.request.headers.forEach((value, key) => {
      headers[key] = value;
    });
    c.state.onWebSocketRequest.requestHeaders = headers;
    websocket.send(
      JSON.stringify({
        hasRequest: true,
        requestUrl: c.request.url,
        requestMethod: c.request.method,
        requestHeaders: headers
      })
    );
    websocket.addEventListener("message", (event19) => {
      websocket.send(event19.data);
    });
  },
  actions: {
    getRequestInfo: (c) => {
      return {
        onBeforeConnect: c.state.onBeforeConnectRequest,
        createConnState: c.state.createConnStateRequest,
        onRequest: c.state.onRequestRequest,
        onWebSocket: c.state.onWebSocketRequest
      };
    }
  }
});

// src/actors/counter/conn-params.ts
import { actor as actor9, event as event3 } from "rivetkit";
var counterWithParams = actor9({
  state: { count: 0, initializers: [] },
  events: {
    newCount: event3()
  },
  createConnState: (_c, params) => {
    return {
      name: params.name || "anonymous"
    };
  },
  onConnect: (c, conn) => {
    c.state.initializers.push(conn.state.name);
  },
  actions: {
    increment: (c, x) => {
      c.state.count += x;
      c.broadcast("newCount", {
        count: c.state.count,
        by: c.conn.state.name
      });
      return c.state.count;
    },
    getInitializers: (c) => {
      return c.state.initializers;
    }
  }
});

// src/actors/counter/counter.ts
import {
  actor as actor10,
  event as event4
} from "rivetkit";
var counter = actor10({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: 5e3
  },
  state: { count: 0 },
  events: {
    newCount: event4()
  },
  onWebSocket(_c, websocket) {
    websocket.addEventListener("message", (event19) => {
      if (websocket.readyState !== 1) return;
      websocket.send(event19.data);
    });
  },
  actions: {
    increment: (c, x) => {
      c.state.count += x;
      c.broadcast("newCount", c.state.count);
      return c.state.count;
    },
    setCount: (c, x) => {
      c.state.count = x;
      c.broadcast("newCount", x);
      return c.state.count;
    },
    getCount: (c) => {
      return c.state.count;
    },
    noop: (_c) => {
      return { ok: true };
    },
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    }
  }
});

// src/actors/counter/counter-conn.ts
import { actor as actor11, event as event5 } from "rivetkit";
var counterConn = actor11({
  state: {
    connectionCount: 0
  },
  events: {
    newCount: event5()
  },
  connState: { count: 0 },
  onConnect: (c, _conn) => {
    c.state.connectionCount += 1;
  },
  onDisconnect: (c, _conn) => {
    c.state.connectionCount -= 1;
  },
  actions: {
    increment: (c, x) => {
      c.conn.state.count += x;
      c.broadcast("newCount", c.conn.state.count);
    },
    setCount: (c, x) => {
      c.conn.state.count = x;
      c.broadcast("newCount", x);
    },
    getCount: (c) => {
      return c.conn.state.count;
    },
    getConnectionCount: (c) => {
      return c.state.connectionCount;
    }
  }
});

// src/actors/counter/lifecycle.ts
import { actor as actor12 } from "rivetkit";
var counterWithLifecycle = actor12({
  state: {
    count: 0,
    events: []
  },
  createConnState: (_c, _params) => ({
    joinTime: Date.now()
  }),
  onWake: (c) => {
    c.state.events.push("onWake");
  },
  onSleep: async (c) => {
    c.state.events.push("onSleep:start");
    await new Promise((resolve2) => setTimeout(resolve2, 1e3));
    c.state.events.push("onSleep:end");
  },
  onBeforeConnect: (c, params) => {
    if (params?.trackLifecycle) c.state.events.push("onBeforeConnect");
  },
  onConnect: (c, conn) => {
    if (conn.params?.trackLifecycle) c.state.events.push("onConnect");
  },
  onDisconnect: (c, conn) => {
    if (conn.params?.trackLifecycle) c.state.events.push("onDisconnect");
  },
  actions: {
    getEvents: (c) => {
      return c.state.events;
    },
    increment: (c, x) => {
      c.state.count += x;
      return c.state.count;
    }
  }
});

// src/actors/counter/ping-pong-counter.ts
import { actor as actor13 } from "rivetkit";
var pingPongCounter = actor13({
  state: {
    pingCount: 0
  },
  onWebSocket(ctx, websocket) {
    websocket.addEventListener("message", (event19) => {
      const data = event19.data;
      if (typeof data !== "string") return;
      let parsed;
      try {
        parsed = JSON.parse(data);
      } catch {
        return;
      }
      if (parsed?.type === "ping") {
        ctx.state.pingCount = ctx.state.pingCount + 1;
        websocket.send(
          JSON.stringify({
            type: "pong",
            pingCount: ctx.state.pingCount,
            timestamp: Date.now()
          })
        );
      }
    });
  },
  actions: {
    getPingCount(c) {
      return c.state.pingCount;
    },
    resetPingCount(c) {
      c.state.pingCount = 0;
      return c.state.pingCount;
    }
  }
});

// src/actors/http/raw-fetch-counter.ts
import { Hono } from "hono";
import { actor as actor14 } from "rivetkit";
var rawFetchCounter = actor14({
  state: {
    count: 0
  },
  createVars: () => {
    return { router: createCounterRouter() };
  },
  onRequest: (c, request) => {
    return c.vars.router.fetch(request, { actor: c });
  },
  actions: {
    // ...actions...
  }
});
function createCounterRouter() {
  const app2 = new Hono();
  app2.get("/count", (c) => {
    const { actor: actor62 } = c.env;
    return c.json({
      count: actor62.state.count
    });
  });
  app2.post("/increment", (c) => {
    const { actor: actor62 } = c.env;
    actor62.state.count++;
    return c.json({
      count: actor62.state.count
    });
  });
  return app2;
}

// src/actors/http/raw-http.ts
import { Hono as Hono2 } from "hono";
import { actor as actor15 } from "rivetkit";
var rawHttpActor = actor15({
  state: {
    requestCount: 0
  },
  onRequest(ctx, request) {
    const url = new URL(request.url);
    const method = request.method;
    ctx.state.requestCount++;
    if (url.pathname === "/api/hello") {
      return new Response(
        JSON.stringify({ message: "Hello from actor!" }),
        {
          headers: { "Content-Type": "application/json" }
        }
      );
    }
    if (url.pathname === "/api/echo" && method === "POST") {
      return new Response(request.body, {
        headers: request.headers
      });
    }
    if (url.pathname === "/api/state") {
      return new Response(
        JSON.stringify({
          requestCount: ctx.state.requestCount
        }),
        {
          headers: { "Content-Type": "application/json" }
        }
      );
    }
    if (url.pathname === "/api/headers") {
      const headers = {};
      request.headers.forEach((value, key) => {
        headers[key] = value;
      });
      return new Response(JSON.stringify(headers), {
        headers: { "Content-Type": "application/json" }
      });
    }
    return new Response("Not Found", { status: 404 });
  },
  actions: {}
});
var rawHttpNoHandlerActor = actor15({
  actions: {}
});
var rawHttpVoidReturnActor = actor15({
  onRequest(_ctx, _request) {
    return void 0;
  },
  actions: {}
});
var rawHttpHonoActor = actor15({
  createVars() {
    const router = new Hono2();
    router.get(
      "/",
      (c) => c.json({ message: "Welcome to Hono actor!" })
    );
    router.get(
      "/users",
      (c) => c.json([
        { id: 1, name: "Alice" },
        { id: 2, name: "Bob" }
      ])
    );
    router.get("/users/:id", (c) => {
      const id = c.req.param("id");
      return c.json({
        id: parseInt(id, 10),
        name: id === "1" ? "Alice" : "Bob"
      });
    });
    router.post("/users", async (c) => {
      const body = await c.req.json();
      return c.json({ id: 3, ...body }, 201);
    });
    router.put("/users/:id", async (c) => {
      const id = c.req.param("id");
      const body = await c.req.json();
      return c.json({ id: parseInt(id, 10), ...body });
    });
    router.delete("/users/:id", (c) => {
      const id = c.req.param("id");
      return c.json({ message: `User ${id} deleted` });
    });
    return { router };
  },
  onRequest(ctx, request) {
    return ctx.vars.router.fetch(request);
  },
  actions: {}
});

// src/actors/http/raw-http-request-properties.ts
import { actor as actor16 } from "rivetkit";
var rawHttpRequestPropertiesActor = actor16({
  actions: {},
  onRequest(_ctx, request) {
    const url = new URL(request.url);
    const method = request.method;
    const headers = {};
    request.headers.forEach((value, key) => {
      headers[key] = value;
    });
    const handleBody = async () => {
      if (!request.body) {
        return null;
      }
      const contentType = request.headers.get("content-type") || "";
      try {
        if (contentType.includes("application/json")) {
          const text2 = await request.text();
          return text2 ? JSON.parse(text2) : null;
        } else {
          const text2 = await request.text();
          return text2 || null;
        }
      } catch (_error) {
        return null;
      }
    };
    if (method === "HEAD") {
      return new Response(null, {
        status: 200
      });
    }
    if (method === "OPTIONS") {
      return new Response(null, {
        status: 204
      });
    }
    return handleBody().then((body) => {
      const responseData = {
        // URL properties
        url: request.url,
        pathname: url.pathname,
        search: url.search,
        searchParams: Object.fromEntries(url.searchParams.entries()),
        hash: url.hash,
        // Method
        method: request.method,
        // Headers
        headers,
        // Body
        body,
        bodyText: typeof body === "string" ? body : body === null && request.body !== null ? "" : null,
        // Additional properties that might be available
        // Note: Some properties like cache, credentials, mode, etc.
        // might not be available in all environments
        cache: request.cache || null,
        credentials: request.credentials || null,
        mode: request.mode || null,
        redirect: request.redirect || null,
        referrer: request.referrer || null
      };
      return new Response(JSON.stringify(responseData), {
        headers: { "Content-Type": "application/json" }
      });
    });
  }
});

// src/actors/http/raw-websocket.ts
import { actor as actor17 } from "rivetkit";
var rawWebSocketActor = actor17({
  state: {
    connectionCount: 0,
    messageCount: 0
  },
  onWebSocket(ctx, websocket) {
    ctx.state.connectionCount = ctx.state.connectionCount + 1;
    console.log(
      `[ACTOR] New connection, count: ${ctx.state.connectionCount}`
    );
    websocket.send(
      JSON.stringify({
        type: "welcome",
        connectionCount: ctx.state.connectionCount
      })
    );
    console.log("[ACTOR] Sent welcome message");
    websocket.addEventListener("message", (event19) => {
      ctx.state.messageCount = ctx.state.messageCount + 1;
      console.log(
        `[ACTOR] Message received, total count: ${ctx.state.messageCount}, data:`,
        event19.data
      );
      const data = event19.data;
      if (typeof data === "string") {
        try {
          const parsed = JSON.parse(data);
          if (parsed.type === "ping") {
            websocket.send(
              JSON.stringify({
                type: "pong",
                timestamp: Date.now()
              })
            );
          } else if (parsed.type === "getStats") {
            console.log(
              `[ACTOR] Sending stats - connections: ${ctx.state.connectionCount}, messages: ${ctx.state.messageCount}`
            );
            websocket.send(
              JSON.stringify({
                type: "stats",
                connectionCount: ctx.state.connectionCount,
                messageCount: ctx.state.messageCount
              })
            );
          } else if (parsed.type === "getRequestInfo") {
            const url = ctx.request?.url || "ws://actor/websocket";
            const urlObj = new URL(url);
            websocket.send(
              JSON.stringify({
                type: "requestInfo",
                url,
                pathname: urlObj.pathname,
                search: urlObj.search
              })
            );
          } else {
            websocket.send(data);
          }
        } catch {
          websocket.send(data);
        }
      } else {
        websocket.send(data);
      }
    });
    websocket.addEventListener("close", () => {
      ctx.state.connectionCount = ctx.state.connectionCount - 1;
      console.log(
        `[ACTOR] Connection closed, count: ${ctx.state.connectionCount}`
      );
    });
  },
  actions: {
    getStats(ctx) {
      return {
        connectionCount: ctx.state.connectionCount,
        messageCount: ctx.state.messageCount
      };
    }
  }
});
var rawWebSocketBinaryActor = actor17({
  onWebSocket(_ctx, websocket) {
    websocket.addEventListener("message", (event19) => {
      const data = event19.data;
      if (data instanceof ArrayBuffer || data instanceof Uint8Array) {
        const bytes = new Uint8Array(data);
        const reversed = new Uint8Array(bytes.length);
        for (let i = 0; i < bytes.length; i++) {
          reversed[i] = bytes[bytes.length - 1 - i];
        }
        websocket.send(reversed);
      }
    });
  },
  actions: {}
});

// src/actors/http/raw-websocket-chat-room.ts
import { actor as actor18 } from "rivetkit";
var rawWebSocketChatRoom = actor18({
  state: {
    messages: []
  },
  createVars: () => {
    return {
      sockets: /* @__PURE__ */ new Set()
    };
  },
  onWebSocket(ctx, socket) {
    ctx.vars.sockets.add(socket);
    socket.send(
      JSON.stringify({
        type: "init",
        messages: ctx.state.messages
      })
    );
    socket.addEventListener("message", (event19) => {
      try {
        const data = JSON.parse(event19.data);
        if (data.type === "message" && data.text) {
          const message = {
            id: crypto.randomUUID(),
            text: data.text,
            timestamp: Date.now()
          };
          ctx.state.messages.push(message);
          ctx.saveState({});
          if (ctx.state.messages.length > 50) {
            ctx.state.messages.shift();
          }
          const broadcast = JSON.stringify({
            type: "message",
            ...message
          });
          for (const ws of ctx.vars.sockets) {
            if (ws.readyState === 1) {
              ws.send(broadcast);
            }
          }
        }
      } catch (e) {
        console.error("Failed to process message:", e);
      }
    });
    socket.addEventListener("close", () => {
      ctx.vars.sockets.delete(socket);
    });
  },
  actions: {}
});

// src/actors/http/raw-websocket-serverless-smoke.ts
import {
  actor as actor19
} from "rivetkit";
function sleep(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, ms));
}
var rawWebSocketServerlessSmoke = actor19({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: 5e3
  },
  state: {
    connectionCount: 0,
    sleepCount: 0,
    totalTickCount: 0,
    totalMessageCount: 0
  },
  async onSleep(c) {
    const delayMs = 10 + Math.floor(Math.random() * 1991);
    c.state.sleepCount += 1;
    c.log.info({
      msg: "raw websocket serverless smoke onSleep delay",
      delayMs,
      sleepCount: c.state.sleepCount
    });
    await sleep(delayMs);
  },
  onWebSocket(c, websocket) {
    c.state.connectionCount += 1;
    const connectionId = crypto.randomUUID();
    let index = 0;
    const sendTick = () => {
      if (websocket.readyState !== 1) return;
      const timestamp = Date.now();
      const message = {
        type: "tick",
        connectionId,
        index,
        timestamp,
        iso: new Date(timestamp).toISOString(),
        totalTickCount: c.state.totalTickCount
      };
      c.state.totalTickCount += 1;
      index += 1;
      websocket.send(JSON.stringify(message));
    };
    c.log.info({
      msg: "raw websocket serverless smoke connected",
      connectionId,
      connectionCount: c.state.connectionCount
    });
    sendTick();
    const interval = setInterval(sendTick, 1e3);
    websocket.addEventListener("message", (event19) => {
      c.state.totalMessageCount += 1;
      c.log.info({
        msg: "raw websocket serverless smoke received message",
        connectionId,
        totalMessageCount: c.state.totalMessageCount
      });
      websocket.send(
        JSON.stringify({
          type: "ack",
          connectionId,
          index,
          timestamp: Date.now(),
          received: event19.data
        })
      );
    });
    websocket.addEventListener("close", () => {
      clearInterval(interval);
      c.state.connectionCount -= 1;
      c.log.info({
        msg: "raw websocket serverless smoke disconnected",
        connectionId,
        connectionCount: c.state.connectionCount
      });
    });
  },
  actions: {
    getStats(c) {
      return {
        connectionCount: c.state.connectionCount,
        sleepCount: c.state.sleepCount,
        totalTickCount: c.state.totalTickCount,
        totalMessageCount: c.state.totalMessageCount
      };
    }
  }
});

// src/actors/http/tunnel-stress.ts
import {
  actor as actor20
} from "rivetkit";
var tunnelStress = actor20({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: 5e3
  },
  state: {
    connectionCount: 0,
    messageCount: 0,
    heartbeatCount: 0
  },
  onWebSocket(c, websocket) {
    c.state.connectionCount += 1;
    const connectionId = crypto.randomUUID();
    const sendHeartbeat = () => {
      if (websocket.readyState !== 1) return;
      c.state.heartbeatCount += 1;
      websocket.send(
        JSON.stringify({
          type: "heartbeat",
          connectionId,
          heartbeatCount: c.state.heartbeatCount,
          timestamp: Date.now()
        })
      );
    };
    const heartbeat = setInterval(sendHeartbeat, 1e3);
    sendHeartbeat();
    websocket.addEventListener(
      "message",
      async (event19) => {
        if (typeof event19.data === "string") {
          let parsed;
          try {
            parsed = JSON.parse(event19.data);
          } catch {
            parsed = void 0;
          }
          if (parsed && typeof parsed === "object" && parsed.type === "ping") {
            const id = parsed.id;
            if (websocket.readyState === 1) {
              websocket.send(
                JSON.stringify({
                  type: "pong",
                  connectionId,
                  id,
                  timestamp: Date.now()
                })
              );
            }
            return;
          }
        }
        c.state.messageCount += 1;
        await c.kv.put("counter", String(c.state.messageCount));
        websocket.send(
          JSON.stringify({
            type: "reply",
            connectionId,
            messageCount: c.state.messageCount,
            timestamp: Date.now(),
            received: event19.data
          })
        );
      }
    );
    websocket.addEventListener("close", () => {
      clearInterval(heartbeat);
      c.state.connectionCount -= 1;
    });
  },
  actions: {
    getStats(c) {
      return {
        connectionCount: c.state.connectionCount,
        messageCount: c.state.messageCount,
        heartbeatCount: c.state.heartbeatCount
      };
    }
  }
});

// src/actors/inter-actor/cross-actor-actions.ts
import { actor as actor21 } from "rivetkit";
var inventory = actor21({
  // Each item has its own inventory actor instance
  createState: (_c, input) => ({
    itemName: input?.itemName ?? "Widget",
    stock: input?.initialStock ?? 100,
    reservations: []
  }),
  actions: {
    // Check current stock
    getStock: (c) => ({
      itemName: c.state.itemName,
      stock: c.state.stock
    }),
    // Reserve items for checkout (called by checkout actor)
    reserveItems: (c, checkoutId, quantity) => {
      if (c.state.stock < quantity) {
        return {
          success: false,
          message: `Insufficient stock. Available: ${c.state.stock}, Requested: ${quantity}`,
          availableStock: c.state.stock
        };
      }
      c.state.stock -= quantity;
      c.state.reservations.push(checkoutId);
      return {
        success: true,
        message: `Reserved ${quantity} items for checkout ${checkoutId}`,
        remainingStock: c.state.stock
      };
    },
    // Release reserved items if checkout is cancelled
    releaseItems: (c, checkoutId, quantity) => {
      const index = c.state.reservations.indexOf(checkoutId);
      if (index > -1) {
        c.state.reservations.splice(index, 1);
        c.state.stock += quantity;
      }
      return {
        success: true,
        remainingStock: c.state.stock
      };
    }
  }
});
var checkout = actor21({
  createState: (_c, input) => ({
    customerName: input?.customerName ?? "Guest",
    items: [],
    completed: false
  }),
  actions: {
    // Add item to checkout and reserve from inventory
    addItem: async (c, itemId, quantity) => {
      const inventoryActor = c.client().inventory.getOrCreate([itemId]);
      const itemInfo = await inventoryActor.getStock();
      const reservation = await inventoryActor.reserveItems(
        c.actorId,
        // Use checkout ID as reservation ID
        quantity
      );
      if (!reservation.success) {
        return {
          success: false,
          message: reservation.message
        };
      }
      c.state.items.push({
        itemId,
        itemName: itemInfo.itemName,
        quantity
      });
      return {
        success: true,
        message: `Added ${quantity} ${itemInfo.itemName} to checkout`,
        remainingStock: reservation.remainingStock
      };
    },
    // Get checkout summary
    getSummary: (c) => ({
      customerName: c.state.customerName,
      items: c.state.items,
      completed: c.state.completed,
      totalItems: c.state.items.reduce(
        (sum, item) => sum + item.quantity,
        0
      )
    }),
    // Complete the checkout
    completeCheckout: (c) => {
      c.state.completed = true;
      return {
        success: true,
        message: "Checkout completed successfully"
      };
    },
    // Cancel checkout and release all reservations
    cancelCheckout: async (c) => {
      for (const item of c.state.items) {
        const inventoryActor = c.client().inventory.getOrCreate([item.itemId]);
        await inventoryActor.releaseItems(c.actorId, item.quantity);
      }
      c.state.items = [];
      return {
        success: true,
        message: "Checkout cancelled, items returned to inventory"
      };
    }
  }
});

// src/actors/lifecycle/destroy.ts
import { actor as actor22 } from "rivetkit";
var destroyObserver = actor22({
  state: { destroyedActors: [] },
  actions: {
    notifyDestroyed: (c, actorKey) => {
      c.state.destroyedActors.push(actorKey);
    },
    wasDestroyed: (c, actorKey) => {
      return c.state.destroyedActors.includes(actorKey);
    },
    reset: (c) => {
      c.state.destroyedActors = [];
    }
  }
});
var destroyActor = actor22({
  state: { value: 0, key: "" },
  onWake: (c) => {
    c.state.key = c.key.join("/");
  },
  onDestroy: async (c) => {
    const client = c.client();
    const observer = client.destroyObserver.getOrCreate(["observer"]);
    await observer.notifyDestroyed(c.state.key);
  },
  actions: {
    setValue: async (c, newValue) => {
      c.state.value = newValue;
      await c.saveState({ immediate: true });
      return c.state.value;
    },
    getValue: (c) => {
      return c.state.value;
    },
    destroy: (c) => {
      c.destroy();
    }
  }
});

// src/actors/lifecycle/hibernation.ts
import { actor as actor23 } from "rivetkit";
var HIBERNATION_SLEEP_TIMEOUT = 500;
var hibernationActor = actor23({
  state: {
    sleepCount: 0,
    wakeCount: 0
  },
  createConnState: (_c) => {
    return {
      count: 0,
      connectCount: 0,
      disconnectCount: 0
    };
  },
  onWake: (c) => {
    c.state.wakeCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  onConnect: (_c, conn) => {
    conn.state.connectCount += 1;
  },
  onDisconnect: (_c, conn) => {
    conn.state.disconnectCount += 1;
  },
  actions: {
    // Basic RPC that returns a simple value
    ping: (_c) => {
      return "pong";
    },
    // Increment the connection's count
    connIncrement: (c) => {
      c.conn.state.count += 1;
      return c.conn.state.count;
    },
    // Get the connection's count
    getConnCount: (c) => {
      return c.conn.state.count;
    },
    // Get the connection's lifecycle counts
    getConnLifecycleCounts: (c) => {
      return {
        connectCount: c.conn.state.connectCount,
        disconnectCount: c.conn.state.disconnectCount
      };
    },
    // Get all connection IDs
    getConnectionIds: (c) => {
      return c.conns.entries().map((x) => x[0]).toArray();
    },
    // Get actor sleep/wake counts
    getActorCounts: (c) => {
      return {
        sleepCount: c.state.sleepCount,
        wakeCount: c.state.wakeCount
      };
    },
    // Trigger sleep
    triggerSleep: (c) => {
      c.sleep();
    }
  },
  options: {
    sleepTimeout: HIBERNATION_SLEEP_TIMEOUT
  }
});

// src/actors/lifecycle/run.ts
import { actor as actor24, queue } from "rivetkit";
var RUN_SLEEP_TIMEOUT = 500;
var runWithTicks = actor24({
  state: {
    tickCount: 0,
    lastTickAt: 0,
    runStarted: false,
    runExited: false
  },
  run: async (c) => {
    c.state.runStarted = true;
    c.log.info("run handler started");
    while (!c.aborted) {
      c.state.tickCount += 1;
      c.state.lastTickAt = Date.now();
      c.log.info({ msg: "tick", tickCount: c.state.tickCount });
      await new Promise((resolve2) => {
        const timeout = setTimeout(resolve2, 50);
        c.abortSignal.addEventListener(
          "abort",
          () => {
            clearTimeout(timeout);
            resolve2();
          },
          { once: true }
        );
      });
    }
    c.state.runExited = true;
    c.log.info("run handler exiting gracefully");
  },
  actions: {
    getState: (c) => ({
      tickCount: c.state.tickCount,
      lastTickAt: c.state.lastTickAt,
      runStarted: c.state.runStarted,
      runExited: c.state.runExited
    })
  },
  options: {
    sleepTimeout: RUN_SLEEP_TIMEOUT
  }
});
var runWithQueueConsumer = actor24({
  state: {
    messagesReceived: [],
    runStarted: false
  },
  queues: {
    messages: queue()
  },
  run: async (c) => {
    c.state.runStarted = true;
    c.log.info("run handler started, waiting for messages");
    for await (const message of c.queue.iter()) {
      c.log.info({ msg: "received message", body: message.body });
      c.state.messagesReceived.push({
        name: message.name,
        body: message.body
      });
    }
    c.log.info("run handler exiting gracefully");
  },
  actions: {
    getState: (c) => ({
      messagesReceived: c.state.messagesReceived,
      runStarted: c.state.runStarted
    }),
    sendMessage: async (c, body) => {
      const client = c.client();
      const handle = client.runWithQueueConsumer.getForId(c.actorId);
      await handle.send("messages", body);
      return true;
    }
  },
  options: {
    sleepTimeout: RUN_SLEEP_TIMEOUT
  }
});
var runWithEarlyExit = actor24({
  state: {
    runStarted: false,
    destroyCalled: false
  },
  run: async (c) => {
    c.state.runStarted = true;
    c.log.info("run handler started, will exit after delay");
    await new Promise((resolve2) => setTimeout(resolve2, 200));
    c.log.info("run handler exiting early");
  },
  onDestroy: (c) => {
    c.state.destroyCalled = true;
  },
  actions: {
    getState: (c) => ({
      runStarted: c.state.runStarted,
      destroyCalled: c.state.destroyCalled
    })
  },
  options: {
    sleepTimeout: RUN_SLEEP_TIMEOUT
  }
});
var runWithError = actor24({
  state: {
    runStarted: false,
    destroyCalled: false
  },
  run: async (c) => {
    c.state.runStarted = true;
    c.log.info("run handler started, will throw error");
    await new Promise((resolve2) => setTimeout(resolve2, 50));
    throw new Error("intentional error in run handler");
  },
  onDestroy: (c) => {
    c.state.destroyCalled = true;
  },
  actions: {
    getState: (c) => ({
      runStarted: c.state.runStarted,
      destroyCalled: c.state.destroyCalled
    })
  },
  options: {
    sleepTimeout: RUN_SLEEP_TIMEOUT
  }
});
var runWithoutHandler = actor24({
  state: {
    wakeCount: 0
  },
  onWake: (c) => {
    c.state.wakeCount += 1;
  },
  actions: {
    getState: (c) => ({
      wakeCount: c.state.wakeCount
    })
  },
  options: {
    sleepTimeout: RUN_SLEEP_TIMEOUT
  }
});

// src/actors/lifecycle/scheduled.ts
import { actor as actor25, event as event6 } from "rivetkit";
var scheduled = actor25({
  state: {
    lastRun: 0,
    scheduledCount: 0,
    taskHistory: []
  },
  events: {
    scheduled: event6(),
    scheduledWithId: event6()
  },
  actions: {
    // Schedule using 'at' with specific timestamp
    scheduleTaskAt: (c, timestamp) => {
      c.schedule.at(timestamp, "onScheduledTask");
      return timestamp;
    },
    // Schedule using 'after' with delay
    scheduleTaskAfter: (c, delayMs) => {
      c.schedule.after(delayMs, "onScheduledTask");
      return Date.now() + delayMs;
    },
    // Schedule with a task ID for ordering tests
    scheduleTaskAfterWithId: (c, taskId, delayMs) => {
      c.schedule.after(delayMs, "onScheduledTaskWithId", taskId);
      return { taskId, scheduledFor: Date.now() + delayMs };
    },
    // Original method for backward compatibility
    scheduleTask: (c, delayMs) => {
      const timestamp = Date.now() + delayMs;
      c.schedule.at(timestamp, "onScheduledTask");
      return timestamp;
    },
    // Getters for state
    getLastRun: (c) => {
      return c.state.lastRun;
    },
    getScheduledCount: (c) => {
      return c.state.scheduledCount;
    },
    getTaskHistory: (c) => {
      return c.state.taskHistory;
    },
    clearHistory: (c) => {
      c.state.taskHistory = [];
      c.state.scheduledCount = 0;
      c.state.lastRun = 0;
      return true;
    },
    // Scheduled task handlers
    onScheduledTask: (c) => {
      c.state.lastRun = Date.now();
      c.state.scheduledCount++;
      c.broadcast("scheduled", {
        time: c.state.lastRun,
        count: c.state.scheduledCount
      });
    },
    onScheduledTaskWithId: (c, taskId) => {
      c.state.lastRun = Date.now();
      c.state.scheduledCount++;
      c.state.taskHistory.push(taskId);
      c.broadcast("scheduledWithId", {
        taskId,
        time: c.state.lastRun,
        count: c.state.scheduledCount
      });
    }
  }
});

// src/actors/lifecycle/sleep.ts
import { actor as actor26, event as event7 } from "rivetkit";
import { promiseWithResolvers } from "rivetkit/utils";
var SLEEP_TIMEOUT = 1e3;
var sleep2 = actor26({
  state: { startCount: 0, sleepCount: 0 },
  onWake: (c) => {
    c.state.startCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  actions: {
    triggerSleep: (c) => {
      c.sleep();
    },
    getCounts: (c) => {
      return {
        startCount: c.state.startCount,
        sleepCount: c.state.sleepCount
      };
    },
    setAlarm: async (c, duration) => {
      await c.schedule.after(duration, "onAlarm");
    },
    onAlarm: (c) => {
      c.log.info("alarm called");
    }
  },
  options: {
    sleepTimeout: SLEEP_TIMEOUT
  }
});
var sleepWithLongRpc = actor26({
  state: { startCount: 0, sleepCount: 0 },
  createVars: () => ({}),
  events: {
    waiting: event7()
  },
  onWake: (c) => {
    c.state.startCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  actions: {
    getCounts: (c) => {
      return {
        startCount: c.state.startCount,
        sleepCount: c.state.sleepCount
      };
    },
    longRunningRpc: async (c) => {
      c.log.info("starting long running rpc");
      c.vars.longRunningResolve = promiseWithResolvers(() => {
      });
      c.broadcast("waiting");
      await c.vars.longRunningResolve.promise;
      c.log.info("finished long running rpc");
    },
    finishLongRunningRpc: (c) => c.vars.longRunningResolve?.resolve()
  },
  options: {
    sleepTimeout: SLEEP_TIMEOUT
  }
});
var sleepWithRawHttp = actor26({
  state: { startCount: 0, sleepCount: 0, requestCount: 0 },
  onWake: (c) => {
    c.state.startCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  onRequest: async (c, request) => {
    c.state.requestCount += 1;
    const url = new URL(request.url);
    if (url.pathname === "/long-request") {
      const duration = parseInt(
        url.searchParams.get("duration") || "1000",
        10
      );
      c.log.info({ msg: "starting long fetch request", duration });
      await new Promise((resolve2) => setTimeout(resolve2, duration));
      c.log.info("finished long fetch request");
      return new Response(JSON.stringify({ completed: true }), {
        headers: { "Content-Type": "application/json" }
      });
    }
    return new Response("Not Found", { status: 404 });
  },
  actions: {
    getCounts: (c) => {
      return {
        startCount: c.state.startCount,
        sleepCount: c.state.sleepCount,
        requestCount: c.state.requestCount
      };
    }
  },
  options: {
    sleepTimeout: SLEEP_TIMEOUT
  }
});
var sleepWithRawWebSocket = actor26({
  state: { startCount: 0, sleepCount: 0, connectionCount: 0 },
  onWake: (c) => {
    c.state.startCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  onWebSocket: (c, websocket) => {
    c.state.connectionCount += 1;
    c.log.info({
      msg: "websocket connected",
      connectionCount: c.state.connectionCount
    });
    websocket.send(
      JSON.stringify({
        type: "connected",
        connectionCount: c.state.connectionCount
      })
    );
    websocket.addEventListener("message", (event19) => {
      const data = event19.data;
      if (typeof data === "string") {
        try {
          const parsed = JSON.parse(data);
          if (parsed.type === "getCounts") {
            websocket.send(
              JSON.stringify({
                type: "counts",
                startCount: c.state.startCount,
                sleepCount: c.state.sleepCount,
                connectionCount: c.state.connectionCount
              })
            );
          } else if (parsed.type === "keepAlive") {
            websocket.send(JSON.stringify({ type: "ack" }));
          }
        } catch {
          websocket.send(data);
        }
      }
    });
    websocket.addEventListener("close", () => {
      c.state.connectionCount -= 1;
      c.log.info({
        msg: "websocket disconnected",
        connectionCount: c.state.connectionCount
      });
    });
  },
  actions: {
    getCounts: (c) => {
      return {
        startCount: c.state.startCount,
        sleepCount: c.state.sleepCount,
        connectionCount: c.state.connectionCount
      };
    }
  },
  options: {
    sleepTimeout: SLEEP_TIMEOUT
  }
});
var sleepWithNoSleepOption = actor26({
  state: { startCount: 0, sleepCount: 0 },
  onWake: (c) => {
    c.state.startCount += 1;
  },
  onSleep: (c) => {
    c.state.sleepCount += 1;
  },
  actions: {
    getCounts: (c) => {
      return {
        startCount: c.state.startCount,
        sleepCount: c.state.sleepCount
      };
    }
  },
  options: {
    sleepTimeout: SLEEP_TIMEOUT,
    noSleep: true
  }
});

// src/actors/queue/worker.ts
import { actor as actor27, event as event8, queue as queue2 } from "rivetkit";
var worker = actor27({
  state: {
    status: "idle",
    processed: 0,
    lastJob: null
  },
  events: {
    statusChanged: event8(),
    jobProcessed: event8()
  },
  queues: {
    jobs: queue2()
  },
  async run(c) {
    c.state.status = "running";
    c.broadcast("statusChanged", {
      status: c.state.status,
      processed: c.state.processed
    });
    for await (const job of c.queue.iter()) {
      c.state.processed += 1;
      c.state.lastJob = job.body;
      c.broadcast("jobProcessed", {
        processed: c.state.processed,
        job: job.body
      });
    }
    c.state.status = "idle";
  },
  actions: {
    getState(c) {
      return {
        status: c.state.status,
        processed: c.state.processed,
        lastJob: c.state.lastJob
      };
    }
  }
});

// src/actors/queue/worker-timeout.ts
import { actor as actor28, event as event9, queue as queue3 } from "rivetkit";
var DEFAULT_TIMEOUT_MS = 2e3;
var workerTimeout = actor28({
  state: {
    status: "idle",
    processed: 0,
    ticks: 0,
    lastTickAt: null,
    lastJob: null,
    timeoutMs: DEFAULT_TIMEOUT_MS
  },
  events: {
    tick: event9(),
    jobProcessed: event9()
  },
  queues: {
    jobs: queue3()
  },
  run: async (c) => {
    c.state.status = "running";
    while (!c.aborted) {
      const message = await c.queue.next({
        names: ["jobs"],
        timeout: c.state.timeoutMs
      });
      if (!message) {
        const at = Date.now();
        c.state.ticks += 1;
        c.state.lastTickAt = at;
        c.broadcast("tick", {
          ticks: c.state.ticks,
          at
        });
        continue;
      }
      c.state.processed += 1;
      c.state.lastJob = message.body;
      c.broadcast("jobProcessed", {
        processed: c.state.processed,
        job: message.body
      });
    }
    c.state.status = "idle";
  },
  actions: {
    enqueueJob: async (c, payload2) => {
      const job = {
        id: crypto.randomUUID(),
        payload: payload2
      };
      await c.queue.send("jobs", job);
      return job;
    },
    setTimeoutMs: (c, timeoutMs) => {
      c.state.timeoutMs = Math.max(100, Math.floor(timeoutMs));
      return c.state.timeoutMs;
    },
    getState: (c) => c.state
  }
});

// src/actors/state/actor-onstatechange.ts
import { actor as actor29 } from "rivetkit";
var onStateChangeActor = actor29({
  state: {
    value: 0
  },
  vars: {
    changeCount: 0
  },
  actions: {
    // Action that modifies state - should trigger onStateChange
    setValue: (c, newValue) => {
      c.state.value = newValue;
      return c.state.value;
    },
    // Action that modifies state multiple times - should trigger onStateChange for each change
    incrementMultiple: (c, times) => {
      for (let i = 0; i < times; i++) {
        c.state.value++;
      }
      return c.state.value;
    },
    // Action that doesn't modify state - should NOT trigger onStateChange
    getValue: (c) => {
      return c.state.value;
    },
    // Action that reads and returns without modifying - should NOT trigger onStateChange
    getDoubled: (c) => {
      const doubled = c.state.value * 2;
      return doubled;
    },
    // Get the count of how many times onStateChange was called
    getChangeCount: (c) => {
      return c.vars.changeCount;
    },
    // Reset change counter for testing
    resetChangeCount: (c) => {
      c.vars.changeCount = 0;
    }
  },
  // Track onStateChange calls
  onStateChange: (c) => {
    c.vars.changeCount++;
  }
});

// src/actors/state/kv.ts
import { actor as actor30 } from "rivetkit";
var kvActor = actor30({
  actions: {
    putText: async (c, key, value) => {
      await c.kv.put(key, value);
      return true;
    },
    getText: async (c, key) => {
      return await c.kv.get(key);
    },
    listText: async (c, prefix) => {
      const results = await c.kv.list(prefix, { keyType: "text" });
      return results.map(([key, value]) => ({
        key,
        value
      }));
    },
    roundtripArrayBuffer: async (c, key, values) => {
      const buffer = new Uint8Array(values).buffer;
      await c.kv.put(key, buffer, { type: "arrayBuffer" });
      const result = await c.kv.get(key, { type: "arrayBuffer" });
      if (!result) {
        return null;
      }
      return Array.from(new Uint8Array(result));
    }
  }
});

// src/actors/state/large-payloads.ts
import { actor as actor31 } from "rivetkit";
var largePayloadActor = actor31({
  state: {},
  actions: {
    /**
     * Accepts a large request payload and returns its size
     */
    processLargeRequest: (_c, data) => {
      return {
        itemCount: data.items.length,
        firstItem: data.items[0],
        lastItem: data.items[data.items.length - 1]
      };
    },
    /**
     * Returns a large response payload
     */
    getLargeResponse: (_c, itemCount) => {
      const items = [];
      for (let i = 0; i < itemCount; i++) {
        items.push(
          `Item ${i} with some additional text to increase size`
        );
      }
      return { items };
    },
    /**
     * Echo back the request data
     */
    echo: (_c, data) => {
      return data;
    }
  }
});
var largePayloadConnActor = actor31({
  state: {},
  connState: {
    lastRequestSize: 0
  },
  actions: {
    /**
     * Accepts a large request payload and returns its size
     */
    processLargeRequest: (c, data) => {
      c.conn.state.lastRequestSize = data.items.length;
      return {
        itemCount: data.items.length,
        firstItem: data.items[0],
        lastItem: data.items[data.items.length - 1]
      };
    },
    /**
     * Returns a large response payload
     */
    getLargeResponse: (_c, itemCount) => {
      const items = [];
      for (let i = 0; i < itemCount; i++) {
        items.push(
          `Item ${i} with some additional text to increase size`
        );
      }
      return { items };
    },
    /**
     * Echo back the request data
     */
    echo: (_c, data) => {
      return data;
    },
    /**
     * Get the last request size
     */
    getLastRequestSize: (c) => {
      return c.conn.state.lastRequestSize;
    }
  }
});

// src/actors/state/metadata.ts
import { actor as actor32 } from "rivetkit";
var metadataActor = actor32({
  state: {
    lastMetadata: null,
    actorName: "",
    // Store tags and region in state for testing since they may not be
    // available in the context in all environments
    storedTags: {},
    storedRegion: null
  },
  onWake: (c) => {
    c.state.actorName = c.name;
  },
  actions: {
    // Set up test tags - this will be called by tests to simulate tags
    setupTestTags: (c, tags) => {
      c.state.storedTags = tags;
      return tags;
    },
    // Set up test region - this will be called by tests to simulate region
    setupTestRegion: (c, region) => {
      c.state.storedRegion = region;
      return region;
    },
    // Get all available metadata
    getMetadata: (c) => {
      const metadata = {
        name: c.name,
        tags: c.state.storedTags,
        region: c.state.storedRegion
      };
      c.state.lastMetadata = metadata;
      return metadata;
    },
    // Get the actor name
    getActorName: (c) => {
      return c.name;
    },
    // Get a specific tag by key
    getTag: (c, key) => {
      return c.state.storedTags[key] || null;
    },
    // Get all tags
    getTags: (c) => {
      return c.state.storedTags;
    },
    // Get the region
    getRegion: (c) => {
      return c.state.storedRegion;
    },
    // Get the stored actor name (from onWake)
    getStoredActorName: (c) => {
      return c.state.actorName;
    },
    // Get last retrieved metadata
    getLastMetadata: (c) => {
      return c.state.lastMetadata;
    }
  }
});

// src/actors/state/parallelism-test.ts
import { actor as actor33, event as event10 } from "rivetkit";
import { db } from "rivetkit/db";
var parallelismTest = actor33({
  state: {
    stateCount: 0
  },
  db: db({
    onMigrate: async (db17) => {
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS counter (
					id INTEGER PRIMARY KEY CHECK (id = 1),
					count INTEGER NOT NULL DEFAULT 0
				)
			`);
      await db17.execute(`
				INSERT OR IGNORE INTO counter (id, count) VALUES (1, 0)
			`);
    }
  }),
  events: {
    stateCountChanged: event10(),
    sqliteCountChanged: event10()
  },
  actions: {
    incrementState: (c) => {
      c.state.stateCount += 1;
      c.broadcast("stateCountChanged", { count: c.state.stateCount });
      return { count: c.state.stateCount };
    },
    getStateCount: (c) => {
      return { count: c.state.stateCount };
    },
    incrementSqlite: async (c) => {
      await c.db.execute(
        `UPDATE counter SET count = count + 1 WHERE id = 1`
      );
      const results = await c.db.execute(
        `SELECT count FROM counter WHERE id = 1`
      );
      const count = results[0].count;
      c.broadcast("sqliteCountChanged", { count });
      return { count };
    },
    getSqliteCount: async (c) => {
      const results = await c.db.execute(
        `SELECT count FROM counter WHERE id = 1`
      );
      return { count: results[0].count };
    }
  },
  options: {
    sleepTimeout: 3e4
  }
});

// src/actors/state/sqlite-drizzle/mod.ts
import { eq } from "drizzle-orm";
import { actor as actor34 } from "rivetkit";
import { db as db2 } from "rivetkit/db/drizzle";

// src/actors/state/sqlite-drizzle/drizzle/meta/_journal.json
var journal_default = {
  version: "7",
  dialect: "sqlite",
  entries: [
    {
      idx: 0,
      version: "6",
      when: 1770921282251,
      tag: "0000_left_wrecking_crew",
      breakpoints: true
    }
  ]
};

// src/actors/state/sqlite-drizzle/drizzle/0000_left_wrecking_crew.sql
var left_wrecking_crew_default = "CREATE TABLE `todos` (\n	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,\n	`title` text NOT NULL,\n	`completed` integer DEFAULT 0,\n	`created_at` integer NOT NULL\n);\n";

// src/actors/state/sqlite-drizzle/drizzle/migrations.js
var migrations_default = {
  journal: journal_default,
  migrations: {
    m0000: left_wrecking_crew_default
  }
};

// src/actors/state/sqlite-drizzle/schema.ts
var schema_exports = {};
__export(schema_exports, {
  todos: () => todos
});
import { integer, sqliteTable, text } from "rivetkit/db/drizzle";
var todos = sqliteTable("todos", {
  id: integer("id").primaryKey({ autoIncrement: true }),
  title: text("title").notNull(),
  completed: integer("completed").default(0),
  createdAt: integer("created_at").notNull()
});

// src/actors/state/sqlite-drizzle/mod.ts
var { todos: todos2 } = schema_exports;
var sqliteDrizzleActor = actor34({
  db: db2({ schema: schema_exports, migrations: migrations_default }),
  actions: {
    addTodo: async (c, title) => {
      const result = await c.db.insert(todos2).values({
        title,
        createdAt: Date.now()
      }).returning();
      return result[0];
    },
    getTodos: async (c) => {
      return await c.db.select().from(todos2).orderBy(todos2.createdAt);
    },
    toggleTodo: async (c, id) => {
      const existing = await c.db.select().from(todos2).where(eq(todos2.id, id));
      if (!existing[0]) return null;
      const newCompleted = existing[0].completed ? 0 : 1;
      const result = await c.db.update(todos2).set({ completed: newCompleted }).where(eq(todos2.id, id)).returning();
      return result[0];
    },
    deleteTodo: async (c, id) => {
      await c.db.delete(todos2).where(eq(todos2.id, id));
      return { deleted: id };
    }
  }
});

// src/actors/state/sqlite-raw.ts
import { actor as actor35 } from "rivetkit";
import { db as db3 } from "rivetkit/db";
var sqliteRawActor = actor35({
  db: db3({
    onMigrate: async (db17) => {
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS todos (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					title TEXT NOT NULL,
					completed INTEGER DEFAULT 0,
					created_at INTEGER NOT NULL
				)
			`);
    }
  }),
  actions: {
    addTodo: async (c, title) => {
      const createdAt = Date.now();
      await c.db.execute(
        "INSERT INTO todos (title, created_at) VALUES (?, ?)",
        title,
        createdAt
      );
      return { title, createdAt };
    },
    getTodos: async (c) => {
      return await c.db.execute(
        "SELECT * FROM todos ORDER BY created_at DESC"
      );
    },
    toggleTodo: async (c, id) => {
      await c.db.execute(
        "UPDATE todos SET completed = NOT completed WHERE id = ?",
        id
      );
      const rows = await c.db.execute(
        "SELECT * FROM todos WHERE id = ?",
        id
      );
      return rows[0];
    },
    deleteTodo: async (c, id) => {
      await c.db.execute("DELETE FROM todos WHERE id = ?", id);
      return { deleted: id };
    }
  }
});

// src/actors/state/vars.ts
import { actor as actor36 } from "rivetkit";
var staticVarActor = actor36({
  state: { value: 0 },
  connState: { hello: "world" },
  vars: { counter: 42, name: "test-actor" },
  actions: {
    getVars: (c) => {
      return c.vars;
    },
    getName: (c) => {
      return c.vars.name;
    }
  }
});
var nestedVarActor = actor36({
  state: { value: 0 },
  connState: { hello: "world" },
  vars: {
    counter: 42,
    nested: {
      value: "original",
      array: [1, 2, 3],
      obj: { key: "value" }
    }
  },
  actions: {
    getVars: (c) => {
      return c.vars;
    },
    modifyNested: (c) => {
      c.vars.nested.value = "modified";
      c.vars.nested.array.push(4);
      c.vars.nested.obj.key = "new-value";
      return c.vars;
    }
  }
});
var dynamicVarActor = actor36({
  state: { value: 0 },
  connState: { hello: "world" },
  createVars: () => {
    return {
      random: Math.random(),
      computed: `Actor-${Math.floor(Math.random() * 1e3)}`
    };
  },
  actions: {
    getVars: (c) => {
      return c.vars;
    }
  }
});
var uniqueVarActor = actor36({
  state: { value: 0 },
  connState: { hello: "world" },
  createVars: () => {
    return {
      id: Math.floor(Math.random() * 1e6)
    };
  },
  actions: {
    getVars: (c) => {
      return c.vars;
    }
  }
});
var driverCtxActor = actor36({
  state: { value: 0 },
  connState: { hello: "world" },
  createVars: (_c, driverCtx) => {
    return {
      hasDriverCtx: Boolean(driverCtx?.isTest)
    };
  },
  actions: {
    getVars: (c) => {
      return c.vars;
    }
  }
});

// src/actors/testing/grow-db.ts
import { actor as actor37 } from "rivetkit";
import { db as db4 } from "rivetkit/db";
var DEFAULT_TARGET_MB = 64;
var DEFAULT_BATCH_ROWS = 128;
var DEFAULT_ROW_BYTES = 8 * 1024;
var DEFAULT_BUDGET_MS = 12e4;
var MEBIBYTE = 1024 * 1024;
function finiteInt(value, fallback) {
  if (value === void 0) return fallback;
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`expected a positive finite number, got ${value}`);
  }
  return Math.floor(value);
}
async function queryOne(database, sql) {
  const rows = await database.execute(sql);
  if (!rows[0]) throw new Error(`query returned no rows: ${sql}`);
  return rows[0];
}
async function storageStats(database) {
  const [pageCount, pageSize, freelistCount] = await Promise.all([
    queryOne(database, "PRAGMA page_count"),
    queryOne(database, "PRAGMA page_size"),
    queryOne(database, "PRAGMA freelist_count")
  ]);
  return {
    pageCount: pageCount.page_count,
    pageSize: pageSize.page_size,
    freelistCount: freelistCount.freelist_count,
    sizeBytes: pageCount.page_count * pageSize.page_size
  };
}
var growDb = actor37({
  options: {
    // Generous per-action timeout. grow() self-limits with budgetMs and
    // returns done=false before this fires.
    actionTimeout: 3e5
  },
  db: db4({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS grow_rows (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					batch INTEGER NOT NULL,
					payload BLOB NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
    }
  }),
  actions: {
    grow: async (c, input = {}) => {
      const startedAt = performance.now();
      const targetMb = finiteInt(input.targetMb, DEFAULT_TARGET_MB);
      const batchRows = finiteInt(input.batchRows, DEFAULT_BATCH_ROWS);
      const rowBytes = finiteInt(input.rowBytes, DEFAULT_ROW_BYTES);
      const budgetMs = finiteInt(input.budgetMs, DEFAULT_BUDGET_MS);
      const targetBytes = targetMb * MEBIBYTE;
      const placeholders = new Array(batchRows).fill("(?, randomblob(?), ?)").join(", ");
      let batchesThisCall = 0;
      let rowsThisCall = 0;
      let stats = await storageStats(c.db);
      while (stats.sizeBytes < targetBytes) {
        if (performance.now() - startedAt >= budgetMs) {
          return {
            ...stats,
            done: false,
            targetBytes,
            batchesThisCall,
            rowsThisCall,
            elapsedMs: Math.round(performance.now() - startedAt)
          };
        }
        const now = Date.now();
        const args = [];
        for (let i = 0; i < batchRows; i += 1) {
          args.push(batchesThisCall, rowBytes, now);
        }
        await c.db.execute("BEGIN");
        try {
          await c.db.execute(
            `INSERT INTO grow_rows (batch, payload, created_at) VALUES ${placeholders}`,
            ...args
          );
          await c.db.execute("COMMIT");
        } catch (err) {
          await c.db.execute("ROLLBACK").catch(() => void 0);
          throw err;
        }
        batchesThisCall += 1;
        rowsThisCall += batchRows;
        stats = await storageStats(c.db);
      }
      return {
        ...stats,
        done: true,
        targetBytes,
        batchesThisCall,
        rowsThisCall,
        elapsedMs: Math.round(performance.now() - startedAt)
      };
    },
    stats: async (c) => storageStats(c.db)
  }
});

// src/actors/testing/inline-client.ts
import { actor as actor38 } from "rivetkit";
function isDynamicSandboxRuntime() {
  return false;
}
async function waitForConnectionOpen(connection) {
  if (connection.connStatus === "connected") {
    return;
  }
  await new Promise((resolve2, reject) => {
    const unsubscribeOpen = connection.onOpen(() => {
      unsubscribeOpen();
      unsubscribeError();
      resolve2();
    });
    const unsubscribeError = connection.onError((error) => {
      unsubscribeOpen();
      unsubscribeError();
      reject(error);
    });
  });
}
var inlineClientActor = actor38({
  state: { messages: [] },
  actions: {
    // Action that uses client to call another actor (stateless)
    callCounterIncrement: async (c, amount) => {
      const client = c.client();
      const result = await client.counter.getOrCreate(["inline-test"]).increment(amount);
      c.state.messages.push(
        `Called counter.increment(${amount}), result: ${result}`
      );
      return result;
    },
    // Action that uses client to get counter state (stateless)
    getCounterState: async (c) => {
      const client = c.client();
      const count = await client.counter.getOrCreate(["inline-test"]).getCount();
      c.state.messages.push(`Got counter state: ${count}`);
      return count;
    },
    // Action that uses client with .connect() for stateful communication
    connectToCounterAndIncrement: async (c, amount) => {
      const client = c.client();
      const handle = client.counter.getOrCreate(["inline-test-stateful"]);
      if (isDynamicSandboxRuntime()) {
        const events2 = [];
        const result12 = await handle.increment(amount);
        events2.push(result12);
        const result22 = await handle.increment(amount * 2);
        events2.push(result22);
        c.state.messages.push(
          `Connected to counter, incremented by ${amount} and ${amount * 2}, results: ${result12}, ${result22}, events: ${JSON.stringify(events2)}`
        );
        return { result1: result12, result2: result22, events: events2 };
      }
      await handle.getCount();
      const connection = handle.connect();
      await waitForConnectionOpen(connection);
      const events = [];
      connection.on("newCount", (count) => {
        events.push(count);
      });
      const result1 = await connection.increment(amount);
      const result2 = await connection.increment(amount * 2);
      await connection.dispose();
      c.state.messages.push(
        `Connected to counter, incremented by ${amount} and ${amount * 2}, results: ${result1}, ${result2}, events: ${JSON.stringify(events)}`
      );
      return { result1, result2, events };
    },
    // Get all messages from this actor's state
    getMessages: (c) => {
      return c.state.messages;
    },
    // Clear messages
    clearMessages: (c) => {
      c.state.messages = [];
    }
  }
});

// src/actors/testing/load-test-agent.ts
import {
  actor as actor39
} from "rivetkit";
import { db as db5 } from "rivetkit/db";
var DEFAULT_TOKENS_PER_SECOND = 20;
var DEFAULT_DURATION_MS = 5e3;
function send(websocket, payload2) {
  if (websocket.readyState !== 1) return;
  websocket.send(JSON.stringify(payload2));
}
function parsePositiveNumber(value, name, fallback) {
  if (value === void 0 || value === null) return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive number`);
  }
  return parsed;
}
function sleep3(ms, signal) {
  if (signal.aborted) return Promise.resolve();
  return new Promise((resolve2) => {
    const timeout = setTimeout(resolve2, ms);
    signal.addEventListener(
      "abort",
      () => {
        clearTimeout(timeout);
        resolve2();
      },
      { once: true }
    );
  });
}
var loadTestAgent = actor39({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: 5e3
  },
  db: db5({
    onMigrate: async (db17) => {
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS messages (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					connection_id TEXT NOT NULL,
					request_id TEXT NOT NULL,
					token_index INTEGER NOT NULL,
					token TEXT NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await db17.execute(`
				CREATE INDEX IF NOT EXISTS messages_request_idx
				ON messages (request_id, token_index)
			`);
    }
  }),
  state: {
    connectionCount: 0,
    inferenceCount: 0,
    tokenCount: 0
  },
  onWebSocket(c, websocket) {
    c.state.connectionCount += 1;
    const connectionId = crypto.randomUUID();
    send(websocket, {
      type: "connected",
      connectionId,
      connectionCount: c.state.connectionCount,
      timestamp: Date.now()
    });
    websocket.addEventListener(
      "message",
      async (event19) => {
        try {
          const message = typeof event19.data === "string" ? JSON.parse(event19.data) : void 0;
          if (message && message.type === "ping") {
            send(websocket, {
              type: "pong",
              connectionId,
              id: message.id,
              timestamp: Date.now()
            });
            return;
          }
          if (!message || message.type !== "inference") {
            throw new Error("expected inference message");
          }
          const requestId = typeof message.requestId === "string" && message.requestId ? message.requestId : crypto.randomUUID();
          const tokensPerSecond = parsePositiveNumber(
            message.tokensPerSecond,
            "tokensPerSecond",
            DEFAULT_TOKENS_PER_SECOND
          );
          const durationMs = parsePositiveNumber(
            message.durationMs,
            "durationMs",
            DEFAULT_DURATION_MS
          );
          const intervalMs = 1e3 / tokensPerSecond;
          const targetTokens = Math.max(
            1,
            Math.floor(durationMs / 1e3 * tokensPerSecond)
          );
          const inference = (async () => {
            c.state.inferenceCount += 1;
            send(websocket, {
              type: "inference-start",
              connectionId,
              requestId,
              tokensPerSecond,
              durationMs,
              targetTokens,
              timestamp: Date.now()
            });
            const startedAt = performance.now();
            for (let i = 0; i < targetTokens; i++) {
              if (c.abortSignal.aborted || websocket.readyState !== 1) {
                break;
              }
              const tokenIndex = i + 1;
              const token = `token-${tokenIndex}`;
              const createdAt = Date.now();
              await c.db.execute(
                "INSERT INTO messages (connection_id, request_id, token_index, token, created_at) VALUES (?, ?, ?, ?, ?)",
                connectionId,
                requestId,
                tokenIndex,
                token,
                createdAt
              );
              c.state.tokenCount += 1;
              send(websocket, {
                type: "token",
                connectionId,
                requestId,
                tokenIndex,
                token,
                timestamp: createdAt
              });
              const nextAt = startedAt + tokenIndex * intervalMs;
              const delayMs = Math.max(
                0,
                nextAt - performance.now()
              );
              if (delayMs > 0) {
                await sleep3(delayMs, c.abortSignal);
              }
            }
            send(websocket, {
              type: "inference-complete",
              connectionId,
              requestId,
              tokenCount: targetTokens,
              timestamp: Date.now()
            });
          })();
          await c.keepAwake(inference);
        } catch (error) {
          send(websocket, {
            type: "error",
            message: error instanceof Error ? error.message : "unknown websocket error",
            timestamp: Date.now()
          });
        }
      }
    );
    websocket.addEventListener("close", () => {
      c.state.connectionCount -= 1;
    });
  },
  actions: {
    getStats(c) {
      return {
        connectionCount: c.state.connectionCount,
        inferenceCount: c.state.inferenceCount,
        tokenCount: c.state.tokenCount
      };
    }
  }
});

// src/actors/testing/load-test-agent-2.ts
import {
  actor as actor40
} from "rivetkit";
import { db as db6 } from "rivetkit/db";
var AsyncMutex = class {
  locked = false;
  waiters = [];
  async acquire() {
    if (!this.locked) {
      this.locked = true;
      return;
    }
    await new Promise((resolve2) => this.waiters.push(resolve2));
    this.locked = true;
  }
  release() {
    const next = this.waiters.shift();
    if (next) {
      next();
      return;
    }
    this.locked = false;
  }
};
function createSerializedDb(execute) {
  const mutex = new AsyncMutex();
  let activeTransaction = null;
  const createTransactionDb = () => {
    const tx = Object.assign(
      (query, ...values) => execute(query, ...values),
      {
        withTransaction: async (_stats, fn) => fn(tx)
      }
    );
    return tx;
  };
  const queryWithMutex = async (query, ...values) => {
    await mutex.acquire();
    try {
      return await execute(query, ...values);
    } finally {
      mutex.release();
    }
  };
  return Object.assign(queryWithMutex, {
    withTransaction: async (stats, fn) => {
      if (activeTransaction) {
        return fn(activeTransaction);
      }
      await mutex.acquire();
      const tx = createTransactionDb();
      try {
        await executeTrackedQuery(
          execute,
          stats,
          "transaction-begin",
          "BEGIN"
        );
        activeTransaction = tx;
        try {
          const result = await fn(tx);
          activeTransaction = null;
          await executeTrackedQuery(
            execute,
            stats,
            "transaction-commit",
            "COMMIT"
          );
          return result;
        } catch (error) {
          activeTransaction = null;
          await executeTrackedQuery(
            execute,
            stats,
            "transaction-rollback",
            "ROLLBACK"
          );
          throw error;
        }
      } finally {
        activeTransaction = null;
        mutex.release();
      }
    }
  });
}
var MESSAGE_COUNT = 84;
var MESSAGE_TOOL_REF_COUNT = 122;
var TOOL_CALL_COUNT = 61;
var EXECUTOR_TOOL_COUNT = 42;
var THREAD_EVENT_COUNT = 233;
var MESSAGE_CONTENT_BYTES = 2600;
var THREAD_EVENT_PAYLOAD_BYTES = 1e3;
var TOOL_CALL_RESULT_BYTES = 2700;
var EXECUTOR_TOOL_SCHEMA_BYTES = 550;
var SLOW_QUERY_MS = 1e3;
function send2(websocket, message) {
  if (websocket.readyState === 1) {
    websocket.send(JSON.stringify(message));
  }
}
var loadTestAgent2 = actor40({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: 5e3
  },
  state: {
    runCount: 0,
    wakeCount: 0,
    queryStats: createAgentConcurrent2QueryStats()
  },
  db: db6({
    onMigrate: async (database) => {
      await createAgentConcurrent2Schema(database);
      await seedAgentConcurrent2Data(database);
    }
  }),
  vars: {
    sql: null,
    wakeStats: null,
    wakeStartedAt: null,
    wakeIteration: 0
  },
  onWebSocket: (c, websocket) => {
    send2(websocket, {
      type: "connected",
      timestamp: Date.now()
    });
    websocket.addEventListener("message", (event19) => {
      const promise = handleAgentConcurrent2Message(
        c,
        websocket,
        event19.data
      );
      void c.keepAwake(promise);
    });
  },
  actions: {
    run: async (c, clientId) => {
      const runtime = ensureAgentConcurrent2Runtime(c);
      c.state.runCount++;
      runtime.vars.wakeIteration++;
      const cycleStats = createAgentConcurrent2QueryStats();
      const stats = createAgentConcurrent2StatsSet(
        cycleStats,
        runtime.wakeStats,
        c.state.queryStats
      );
      const result = await runAgentConcurrent2Workload(
        runtime.sql,
        clientId ?? `agent2-action-${c.state.runCount}`,
        0,
        stats
      );
      return {
        ...result,
        stats: snapshotAgentConcurrent2Stats(c, cycleStats)
      };
    },
    getRunCount: (c) => c.state.runCount,
    sleep: (c) => {
      c.sleep();
      return true;
    }
  }
});
async function handleAgentConcurrent2Message(c, websocket, data) {
  let trigger = "unknown";
  let cycleStats = null;
  try {
    const request = parseAgentConcurrent2Request(data);
    trigger = request.type;
    if (request.type === "ping") {
      send2(websocket, {
        type: "pong",
        id: request.id,
        timestamp: Date.now()
      });
      return;
    }
    if (request.type === "force_sleep") {
      send2(websocket, { type: "sleeping", timestamp: Date.now() });
      c.sleep();
      return;
    }
    const runtime = ensureAgentConcurrent2Runtime(c);
    c.state.runCount++;
    runtime.vars.wakeIteration++;
    cycleStats = createAgentConcurrent2QueryStats();
    const stats = createAgentConcurrent2StatsSet(
      cycleStats,
      runtime.wakeStats,
      c.state.queryStats
    );
    if (request.type === "agent2_resume") {
      const startedAt = performance.now();
      const result2 = await runCatchupSnapshot(
        runtime.sql,
        request.version,
        stats
      );
      send2(websocket, {
        type: "agent2_result",
        trigger: request.type,
        totalMs: Math.round(performance.now() - startedAt),
        results: [result2],
        stats: snapshotAgentConcurrent2Stats(c, cycleStats)
      });
      return;
    }
    const result = await runAgentConcurrent2Workload(
      runtime.sql,
      request.clientId,
      request.staggerHandleMs ?? 0,
      stats
    );
    send2(websocket, {
      type: "agent2_result",
      trigger: request.type,
      ...result,
      stats: snapshotAgentConcurrent2Stats(c, cycleStats)
    });
  } catch (error) {
    send2(websocket, {
      type: "agent2_error",
      trigger,
      error: error instanceof Error ? error.message : String(error),
      ...cycleStats ? { stats: snapshotAgentConcurrent2Stats(c, cycleStats) } : {}
    });
  }
}
function parseAgentConcurrent2Request(data) {
  if (typeof data !== "string") {
    throw new Error("agent concurrent 2 request must be a string");
  }
  const parsed = JSON.parse(data);
  if (!parsed || typeof parsed !== "object") {
    throw new Error("agent concurrent 2 request must be an object");
  }
  const request = parsed;
  if (request.type === "ping") {
    return {
      type: "ping",
      ...typeof request.id === "number" ? { id: request.id } : {}
    };
  }
  if (request.type === "force_sleep") {
    return { type: "force_sleep" };
  }
  if (request.type === "agent2_resume") {
    return {
      type: "agent2_resume",
      version: numberField(request, "version")
    };
  }
  if (request.type === "agent2_connect") {
    return {
      type: "agent2_connect",
      clientId: stringField(request, "clientId"),
      ...typeof request.staggerHandleMs === "number" ? { staggerHandleMs: request.staggerHandleMs } : {}
    };
  }
  throw new Error(
    `unknown agent concurrent 2 request type: ${String(request.type)}`
  );
}
function stringField(record, field) {
  const value = record[field];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`agent concurrent 2 request ${field} must be a string`);
  }
  return value;
}
function numberField(record, field) {
  const value = record[field];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(
      `agent concurrent 2 request ${field} must be a finite number`
    );
  }
  return value;
}
function createAgentConcurrent2Db(db17) {
  return createSerializedDb(
    async (query, ...values) => {
      const converted = values.map(
        (value) => typeof value === "boolean" ? value ? 1 : 0 : value
      );
      return await db17.execute(query, ...converted);
    }
  );
}
function ensureAgentConcurrent2Runtime(c) {
  c.vars.sql ??= createAgentConcurrent2Db(c.db);
  c.state.queryStats ??= createAgentConcurrent2QueryStats();
  c.state.wakeCount ??= 0;
  if (!c.vars.wakeStats) {
    c.vars.wakeStats = createAgentConcurrent2QueryStats();
    c.vars.wakeStartedAt = Date.now();
    c.vars.wakeIteration = 0;
    c.state.wakeCount++;
  }
  return {
    sql: c.vars.sql,
    wakeStats: c.vars.wakeStats,
    vars: c.vars
  };
}
function createAgentConcurrent2QueryStats() {
  return {
    total: 0,
    reads: 0,
    mutations: 0,
    tx: 0,
    other: 0,
    rows: 0,
    errors: 0,
    slow: 0,
    maxMs: 0,
    maxStep: "",
    byOperation: {},
    byTable: {}
  };
}
function createAgentConcurrent2StatsSet(cycle, wake, actor62) {
  return { cycle, wake, actor: actor62 };
}
function snapshotAgentConcurrent2Stats(c, cycle) {
  return {
    wakeIndex: c.state.wakeCount,
    actorIteration: c.state.runCount,
    wakeIteration: c.vars.wakeIteration,
    cycle: cloneAgentConcurrent2QueryStats(cycle),
    wake: cloneAgentConcurrent2QueryStats(
      c.vars.wakeStats ?? createAgentConcurrent2QueryStats()
    ),
    actor: cloneAgentConcurrent2QueryStats(c.state.queryStats)
  };
}
function cloneAgentConcurrent2QueryStats(stats) {
  return {
    total: stats.total,
    reads: stats.reads,
    mutations: stats.mutations,
    tx: stats.tx,
    other: stats.other,
    rows: stats.rows,
    errors: stats.errors,
    slow: stats.slow,
    maxMs: stats.maxMs,
    maxStep: stats.maxStep,
    byOperation: { ...stats.byOperation },
    byTable: { ...stats.byTable }
  };
}
async function runAgentConcurrent2Workload(sql, clientId, staggerHandleMs, stats) {
  const startedAt = performance.now();
  const buildToolPlanContext = runBuildToolPlanContext(sql, stats);
  const catchupSnapshot = runCatchupSnapshot(sql, 0, stats);
  const recoverToolCalls = runRecoverToolCalls(sql, stats);
  const mutationMix = runMutationMix(sql, clientId, stats);
  const handleExecutorConnect = delay(staggerHandleMs).then(
    () => runHandleClientConnect(sql, clientId, stats)
  );
  const results = await Promise.all([
    handleExecutorConnect,
    buildToolPlanContext,
    catchupSnapshot,
    recoverToolCalls,
    mutationMix
  ]);
  return {
    totalMs: Math.round(performance.now() - startedAt),
    results
  };
}
async function runHandleClientConnect(sql, clientId, stats) {
  const startedAt = performance.now();
  const steps = [];
  const nextSeq = await sql.withTransaction(stats, async (tx) => {
    const latestExecutor = await timedQuery(
      tx,
      stats,
      steps,
      "load-latest-executor-id",
      `SELECT executor_id FROM executor_tools ORDER BY updated_at DESC LIMIT 1`
    );
    const latestExecutorId = String(
      latestExecutor[0]?.executor_id ?? "seed-executor"
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "select-cached-executor-tools",
      `SELECT tool_name, schema FROM executor_tools WHERE executor_id = ? ORDER BY tool_name ASC`,
      latestExecutorId
    );
    const executorType = await timedQuery(
      tx,
      stats,
      steps,
      "select-executor-type",
      `SELECT value FROM thread_meta_kv WHERE key = 'executor_type'`
    );
    if (!executorType[0]?.value) {
      await timedQuery(
        tx,
        stats,
        steps,
        "set-executor-type",
        `INSERT OR REPLACE INTO thread_meta_kv (key, value, updated_at) VALUES ('executor_type', ?, ?)`,
        "local-client",
        (/* @__PURE__ */ new Date()).toISOString()
      );
    }
    const sandboxIntent = await timedQuery(
      tx,
      stats,
      steps,
      "select-workspace-intent",
      `SELECT value FROM thread_meta_kv WHERE key = 'workspace_intent'`
    );
    if (hasPendingLaunch(sandboxIntent[0]?.value)) {
      await timedQuery(
        tx,
        stats,
        steps,
        "clear-pending-launch",
        `UPDATE thread_meta_kv SET value = ?, updated_at = ? WHERE key = 'workspace_intent'`,
        JSON.stringify({ spec: null, pendingLaunch: null }),
        (/* @__PURE__ */ new Date()).toISOString()
      );
    }
    const seqRows = await timedQuery(
      tx,
      stats,
      steps,
      "select-next-thread-event-seq",
      `SELECT COALESCE(MAX(seq), 0) + 1 AS seq FROM thread_events`
    );
    const seq = Number(seqRows[0]?.seq ?? 1);
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-client-connected-event",
      `INSERT INTO thread_events (seq, event_type, payload, created_at) VALUES (?, ?, ?, ?)`,
      seq,
      "client_connected",
      JSON.stringify({ type: "client_connected", clientId }),
      (/* @__PURE__ */ new Date()).toISOString()
    );
    return seq;
  });
  steps.push({
    name: "transaction-total",
    durationMs: Math.round(performance.now() - startedAt),
    rowCount: nextSeq
  });
  return {
    name: "handle-client-connect",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runBuildToolPlanContext(sql, stats) {
  const startedAt = performance.now();
  const steps = [];
  const latestExecutor = await timedQuery(
    sql,
    stats,
    steps,
    "load-latest-executor-id",
    `SELECT executor_id FROM executor_tools ORDER BY updated_at DESC LIMIT 1`
  );
  const latestExecutorId = String(
    latestExecutor[0]?.executor_id ?? "seed-executor"
  );
  await timedQuery(
    sql,
    stats,
    steps,
    "select-executor-tools",
    `SELECT tool_name, schema FROM executor_tools WHERE executor_id = ? ORDER BY tool_name ASC`,
    latestExecutorId
  );
  await timedQuery(
    sql,
    stats,
    steps,
    "count-uncancelled-top-level",
    `SELECT COUNT(*) as count FROM messages WHERE cancelled = 0 AND parent_tool_use_id IS NULL`
  );
  const unresolvedRows = await timedQuery(
    sql,
    stats,
    steps,
    "find-unresolved-assistant-message",
    `SELECT m.*
			FROM message_tool_refs AS tool_use
			JOIN messages AS m
				ON m.message_id = tool_use.assistant_message_id
			WHERE tool_use.block_type = 'tool_use'
				AND tool_use.cancelled = 0
				AND m.cancelled = 0
				AND m.role = 'assistant'
				AND m.parent_tool_use_id IS NULL
				AND NOT EXISTS (
					SELECT 1
					FROM message_tool_refs AS tool_result
					JOIN messages AS tool_result_message
						ON tool_result_message.message_id = tool_result.source_message_id
					WHERE tool_result.assistant_message_id = tool_use.assistant_message_id
						AND tool_result.block_type = 'tool_result'
						AND tool_result.cancelled = 0
						AND tool_result.tool_use_id = tool_use.tool_use_id
						AND tool_result_message.parent_tool_use_id IS NULL
				)
			GROUP BY m.message_id
			ORDER BY m.created_at DESC
			LIMIT 1`
  );
  const unresolvedMessageId = unresolvedRows[0]?.message_id;
  if (typeof unresolvedMessageId === "string") {
    await timedQuery(
      sql,
      stats,
      steps,
      "get-persisted-tool-result-ids",
      `SELECT tool_result.tool_use_id
				FROM message_tool_refs AS tool_result
				JOIN messages AS tool_result_message
					ON tool_result_message.message_id = tool_result.source_message_id
				WHERE tool_result.assistant_message_id = ?
					AND tool_result.block_type = 'tool_result'
					AND tool_result.cancelled = 0
					AND tool_result_message.parent_tool_use_id IS NULL`,
      unresolvedMessageId
    );
    await timedQuery(
      sql,
      stats,
      steps,
      "get-tool-calls-by-message-id",
      `SELECT * FROM tool_calls WHERE message_id = ?`,
      unresolvedMessageId
    );
  }
  await timedQuery(
    sql,
    stats,
    steps,
    "is-last-message-cancelled-assistant",
    `SELECT role, cancelled FROM messages
			WHERE parent_tool_use_id IS NULL
			ORDER BY created_at DESC
			LIMIT 1`
  );
  await timedQuery(
    sql,
    stats,
    steps,
    "get-last-uncancelled",
    `SELECT m.* FROM messages m
			WHERE m.cancelled = 0 AND m.parent_tool_use_id IS NULL
			ORDER BY m.created_at DESC
			LIMIT 1`
  );
  return {
    name: "build-tool-plan-context",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runCatchupSnapshot(sql, version, stats) {
  const startedAt = performance.now();
  const steps = [];
  await Promise.all([
    timedQuery(
      sql,
      stats,
      steps,
      "thread-events-list-since-version",
      `SELECT seq, event_type, payload, created_at FROM thread_events WHERE seq > ? ORDER BY seq ASC`,
      version
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "environment-snapshot",
      `SELECT snapshot FROM environment_snapshot WHERE id = 1`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "thread-settings-snapshot",
      `SELECT settings FROM thread_settings_snapshot WHERE id = 1`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "retry-state",
      `SELECT * FROM retry_state WHERE id = 1`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "queued-messages",
      `SELECT * FROM queued_messages ORDER BY created_at ASC`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "executor-artifacts",
      `SELECT artifact_key, data_type, length(content_base64) AS bytes, tool_call_id, updated_at FROM executor_artifacts ORDER BY updated_at ASC`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "tool-approvals",
      `SELECT * FROM tool_approvals ORDER BY timestamp ASC`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "compaction-summaries",
      `SELECT cut_message_id, created_at FROM compaction_summaries ORDER BY created_at ASC`
    ),
    timedQuery(
      sql,
      stats,
      steps,
      "executor-status",
      `SELECT value FROM thread_meta_kv WHERE key = 'executor_status'`
    )
  ]);
  steps.sort((a, b) => b.durationMs - a.durationMs);
  return {
    name: "catchup-snapshot",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runRecoverToolCalls(sql, stats) {
  const startedAt = performance.now();
  const steps = [];
  await timedQuery(
    sql,
    stats,
    steps,
    "hydrate-tool-progress",
    `SELECT id, progress
			FROM tool_calls
			WHERE progress IS NOT NULL
				AND state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  await timedQuery(
    sql,
    stats,
    steps,
    "get-pending-tool-calls",
    `SELECT * FROM tool_calls
			WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')
			ORDER BY issued_at ASC`
  );
  await timedQuery(
    sql,
    stats,
    steps,
    "get-next-tool-expiry",
    `SELECT MIN(expires_at) AS expires_at
			FROM tool_calls
			WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  return {
    name: "recover-tool-calls",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runMutationMix(sql, clientId, stats) {
  const startedAt = performance.now();
  const steps = [];
  const writeCount = await sql.withTransaction(stats, async (tx) => {
    const now = (/* @__PURE__ */ new Date()).toISOString();
    const suffix = safeId(clientId);
    const seqRows = await timedQuery(
      tx,
      stats,
      steps,
      "select-max-thread-event-seq",
      `SELECT COALESCE(MAX(seq), 0) + 1 AS seq FROM thread_events`
    );
    const seq = Number(seqRows[0]?.seq ?? 1);
    const lastMessageRows = await timedQuery(
      tx,
      stats,
      steps,
      "select-last-message-created-at",
      `SELECT MAX(created_at) AS created_at FROM messages`
    );
    const latestToolRows = await timedQuery(
      tx,
      stats,
      steps,
      "select-existing-tool-call",
      `SELECT id FROM tool_calls ORDER BY issued_at DESC LIMIT 1`
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "select-sandbox-row",
      `SELECT sandbox_id, restart_attempts, traffic_access_token, project_id, repository_url, additional_repositories, setup
				FROM e2b_sandbox
				WHERE id = 1`
    );
    const messageIdValue = `agent2-message-${suffix}-${seq}`;
    const toolUseIdValue = `agent2-tool-${suffix}-${seq}`;
    const toolCallIdValue = `agent2-call-${suffix}-${seq}`;
    const latestToolCallId = String(latestToolRows[0]?.id ?? toolUseID(1));
    const lastCreatedAt = String(lastMessageRows[0]?.created_at ?? now);
    await timedQuery(
      tx,
      stats,
      steps,
      "upsert-agent-state",
      `INSERT OR REPLACE INTO thread_meta_kv (key, value, updated_at) VALUES (?, ?, ?)`,
      "last_agent_state",
      JSON.stringify({ status: "working", clientId, lastCreatedAt }),
      now
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-work-event",
      `INSERT INTO thread_events (seq, event_type, payload, created_at) VALUES (?, ?, ?, ?)`,
      seq,
      "message_added",
      JSON.stringify({
        type: "message_added",
        messageId: messageIdValue
      }),
      now
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-message",
      `INSERT INTO messages (role, content, meta, user_state, message_id, created_at, cancelled, parent_tool_use_id, tool_result_for_message_id)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      "assistant",
      "agent concurrent 2 mutation payload",
      JSON.stringify({ clientId, seq }),
      null,
      messageIdValue,
      now,
      0,
      null,
      null
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "delete-message-tool-refs",
      `DELETE FROM message_tool_refs WHERE source_message_id = ?`,
      messageIdValue
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-message-added-event",
      `INSERT OR IGNORE INTO message_added_events (message_id, seq) VALUES (?, ?)`,
      messageIdValue,
      seq
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-message-tool-ref",
      `INSERT INTO message_tool_refs (source_message_id, assistant_message_id, tool_use_id, block_type, cancelled)
				VALUES (?, ?, ?, ?, ?)`,
      messageIdValue,
      messageIdValue,
      toolUseIdValue,
      "tool_use",
      0
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "insert-tool-call",
      `INSERT OR IGNORE INTO tool_calls (id, provider_tool_use_id, tool_name, args, executor_id, message_id, issued_at, expires_at, state, result, progress, completed_at)
				VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      toolCallIdValue,
      `provider-${toolCallIdValue}`,
      "tool_1",
      JSON.stringify({ path: `/tmp/${toolCallIdValue}` }),
      "seed-executor",
      messageIdValue,
      now,
      null,
      "running",
      null,
      JSON.stringify({ pct: 0.5, clientId }),
      null
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "update-tool-call-progress",
      `UPDATE tool_calls SET progress = ? WHERE id = ? AND state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`,
      JSON.stringify({ pct: 0.75, clientId, updatedAt: now }),
      toolCallIdValue
    );
    await timedQuery(
      tx,
      stats,
      steps,
      "update-existing-tool-call-progress",
      `UPDATE tool_calls SET progress = ? WHERE id = ? AND state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`,
      JSON.stringify({ pct: 0.25, clientId, updatedAt: now }),
      latestToolCallId
    );
    return seq;
  });
  steps.push({
    name: "transaction-total",
    durationMs: Math.round(performance.now() - startedAt),
    rowCount: writeCount
  });
  return {
    name: "mutation-mix",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function timedQuery(sql, stats, steps, name, query, ...values) {
  const startedAt = performance.now();
  try {
    const rows = await sql(query, ...values);
    const durationMs = Math.round(performance.now() - startedAt);
    recordAgentConcurrent2Query(
      stats,
      name,
      query,
      durationMs,
      rows.length,
      false
    );
    steps.push({
      name,
      durationMs,
      rowCount: rows.length
    });
    return rows;
  } catch (error) {
    const durationMs = Math.round(performance.now() - startedAt);
    recordAgentConcurrent2Query(stats, name, query, durationMs, 0, true);
    throw error;
  }
}
async function executeTrackedQuery(execute, stats, name, query, ...values) {
  const startedAt = performance.now();
  try {
    const rows = await execute(query, ...values);
    recordAgentConcurrent2Query(
      stats,
      name,
      query,
      Math.round(performance.now() - startedAt),
      rows.length,
      false
    );
    return rows;
  } catch (error) {
    recordAgentConcurrent2Query(
      stats,
      name,
      query,
      Math.round(performance.now() - startedAt),
      0,
      true
    );
    throw error;
  }
}
function recordAgentConcurrent2Query(stats, name, query, durationMs, rowCount, failed) {
  const classification = classifyAgentConcurrent2Query(query);
  for (const target of [stats.cycle, stats.wake, stats.actor]) {
    target.total++;
    target.rows += rowCount;
    if (failed) target.errors++;
    if (durationMs >= SLOW_QUERY_MS) target.slow++;
    if (durationMs > target.maxMs) {
      target.maxMs = durationMs;
      target.maxStep = `${name}:${classification.table}`;
    }
    target.byOperation[classification.operation] = (target.byOperation[classification.operation] ?? 0) + 1;
    target.byTable[classification.table] = (target.byTable[classification.table] ?? 0) + 1;
    if (classification.kind === "read") {
      target.reads++;
    } else if (classification.kind === "mutation") {
      target.mutations++;
    } else if (classification.kind === "tx") {
      target.tx++;
    } else {
      target.other++;
    }
  }
}
function classifyAgentConcurrent2Query(query) {
  const normalized = query.trim().replace(/\s+/g, " ");
  const operation = normalized.match(/^([a-z]+)/i)?.[1]?.toLowerCase() ?? "other";
  const table = extractAgentConcurrent2Table(normalized, operation);
  if (operation === "select") {
    return { operation, kind: "read", table };
  }
  if (operation === "insert" || operation === "update" || operation === "delete" || operation === "replace") {
    return { operation, kind: "mutation", table };
  }
  if (operation === "begin" || operation === "commit" || operation === "rollback") {
    return { operation, kind: "tx", table };
  }
  return { operation, kind: "other", table };
}
function extractAgentConcurrent2Table(query, operation) {
  const lower = query.toLowerCase();
  if (operation === "select") {
    return firstMatch(lower, /\bfrom\s+([a-z0-9_]+)/) ?? "unknown";
  }
  if (operation === "insert" || operation === "replace") {
    return firstMatch(lower, /\binto\s+([a-z0-9_]+)/) ?? "unknown";
  }
  if (operation === "update") {
    return firstMatch(lower, /\bupdate\s+([a-z0-9_]+)/) ?? "unknown";
  }
  if (operation === "delete") {
    return firstMatch(lower, /\bfrom\s+([a-z0-9_]+)/) ?? "unknown";
  }
  if (operation === "begin" || operation === "commit" || operation === "rollback") {
    return "transaction";
  }
  return "unknown";
}
function firstMatch(value, pattern) {
  return pattern.exec(value)?.[1] ?? null;
}
function hasPendingLaunch(value) {
  if (typeof value !== "string" || value.length === 0) {
    return false;
  }
  try {
    const parsed = JSON.parse(value);
    return parsed.pendingLaunch !== null && parsed.pendingLaunch !== void 0;
  } catch {
    return false;
  }
}
function delay(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, Math.max(0, ms)));
}
async function createAgentConcurrent2Schema(database) {
  await database.execute(`CREATE TABLE IF NOT EXISTS executor_tools (
		executor_id TEXT NOT NULL,
		tool_name TEXT NOT NULL,
		schema TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		PRIMARY KEY (executor_id, tool_name)
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_executor_tools_executor ON executor_tools(executor_id)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS thread_meta_kv (
		key TEXT PRIMARY KEY,
		value TEXT,
		updated_at TEXT NOT NULL
	)`);
  await database.execute(`CREATE TABLE IF NOT EXISTS thread_events (
		seq INTEGER PRIMARY KEY,
		event_type TEXT NOT NULL,
		payload TEXT NOT NULL,
		created_at TEXT NOT NULL DEFAULT (datetime('now'))
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_thread_events_seq ON thread_events(seq)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS message_added_events (
		message_id TEXT PRIMARY KEY,
		seq INTEGER NOT NULL UNIQUE
	)`);
  await database.execute(`CREATE TABLE IF NOT EXISTS messages (
		message_id TEXT PRIMARY KEY,
		role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'info')),
		content TEXT NOT NULL,
		meta TEXT,
		user_state TEXT,
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
		cancelled INTEGER NOT NULL DEFAULT 0,
		read_at TEXT,
		parent_tool_use_id TEXT,
		tool_result_for_message_id TEXT
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_role_cancelled_created_at ON messages(parent_tool_use_id, role, cancelled, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_cancelled_created_at ON messages(parent_tool_use_id, cancelled, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_created_at ON messages(parent_tool_use_id, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_role_created_at ON messages(role, created_at)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS message_tool_refs (
		source_message_id TEXT NOT NULL,
		assistant_message_id TEXT NOT NULL,
		tool_use_id TEXT NOT NULL,
		block_type TEXT NOT NULL CHECK(block_type IN ('tool_use', 'tool_result')),
		cancelled INTEGER NOT NULL DEFAULT 0,
		PRIMARY KEY (source_message_id, block_type, tool_use_id)
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_assistant_lookup ON message_tool_refs(assistant_message_id, block_type, cancelled, tool_use_id)`
  );
  await database.execute(
    `CREATE UNIQUE INDEX IF NOT EXISTS idx_message_tool_refs_live_tool_result ON message_tool_refs(assistant_message_id, tool_use_id) WHERE block_type = 'tool_result' AND cancelled = 0`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_source_message ON message_tool_refs(source_message_id)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_tool_use_lookup ON message_tool_refs(tool_use_id, assistant_message_id) WHERE block_type = 'tool_use' AND cancelled = 0`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS tool_calls (
		id TEXT PRIMARY KEY,
		provider_tool_use_id TEXT NOT NULL,
		tool_name TEXT NOT NULL,
		args TEXT NOT NULL,
		executor_id TEXT,
		message_id TEXT NOT NULL,
		issued_at TEXT NOT NULL,
		expires_at TEXT,
		state TEXT NOT NULL CHECK(state IN ('queued', 'pending_reconnect', 'pending_ack', 'running', 'completed', 'expired', 'revoked')),
		result TEXT,
		progress TEXT,
		completed_at TEXT
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_message_id ON tool_calls(message_id)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_state ON tool_calls(state)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_expires_at ON tool_calls(expires_at) WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS environment_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), snapshot TEXT NOT NULL, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS thread_settings_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), settings TEXT NOT NULL, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS retry_state (id INTEGER PRIMARY KEY CHECK (id = 1), attempt INTEGER NOT NULL DEFAULT 0, scheduled_at INTEGER NOT NULL, reason TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS queued_messages (message_id TEXT PRIMARY KEY, content TEXT NOT NULL, user_state TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), steer INTEGER NOT NULL DEFAULT 0, user_meta TEXT)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS executor_artifacts (artifact_key TEXT PRIMARY KEY, data_type TEXT NOT NULL, content_base64 TEXT NOT NULL, tool_call_id TEXT, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS e2b_sandbox (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			sandbox_id TEXT,
			restart_attempts INTEGER NOT NULL DEFAULT 0,
			traffic_access_token TEXT,
			project_id TEXT,
			repository_url TEXT,
			additional_repositories TEXT,
			setup TEXT,
			created_at TEXT NOT NULL,
			updated_at TEXT NOT NULL
		)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS tool_approvals (id TEXT PRIMARY KEY, tool_call_id TEXT NOT NULL UNIQUE, tool_name TEXT NOT NULL, args TEXT NOT NULL, reason TEXT, to_allow TEXT, context TEXT NOT NULL CHECK(context IN ('thread', 'subagent')), subagent_tool_name TEXT, parent_tool_call_id TEXT, timestamp INTEGER NOT NULL, matched_rule TEXT, rule_source TEXT CHECK(rule_source IN ('user', 'built-in')))`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_approvals_timestamp ON tool_approvals(timestamp)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS compaction_summaries (summary_id TEXT PRIMARY KEY, summary_text TEXT NOT NULL, cut_message_id TEXT NOT NULL, created_at TEXT NOT NULL)`
  );
}
async function seedAgentConcurrent2Data(database) {
  const existing = await database.execute(
    `SELECT COUNT(*) AS count FROM messages`
  );
  if (Number(existing[0]?.count ?? 0) > 0) {
    return;
  }
  const now = (/* @__PURE__ */ new Date("2026-05-16T03:58:18.661Z")).getTime();
  const text2 = (size) => "x".repeat(size);
  const isoAt = (index) => new Date(now + index * 1e3).toISOString();
  await batchInsert(
    database,
    `INSERT INTO thread_meta_kv (key, value, updated_at)`,
    [
      ["executor_type", "local-client", isoAt(0)],
      [
        "workspace_intent",
        JSON.stringify({ spec: null, pendingLaunch: null }),
        isoAt(0)
      ],
      [
        "executor_status",
        JSON.stringify({ available: true, message: "ready" }),
        isoAt(0)
      ]
    ]
  );
  const messageRows = [];
  for (let index = 1; index <= MESSAGE_COUNT; index++) {
    const role = index % 2 === 0 ? "assistant" : "user";
    messageRows.push([
      messageId(index),
      role,
      text2(MESSAGE_CONTENT_BYTES),
      null,
      null,
      isoAt(index),
      0,
      null,
      null,
      null
    ]);
  }
  await batchInsert(
    database,
    `INSERT INTO messages (message_id, role, content, meta, user_state, created_at, cancelled, read_at, parent_tool_use_id, tool_result_for_message_id)`,
    messageRows,
    20
  );
  const messageToolRefRows = [];
  for (let index = 0; index < MESSAGE_TOOL_REF_COUNT / 2; index++) {
    const assistantIndex = 2 + index % 42 * 2;
    const sourceIndex = Math.max(1, assistantIndex - 1);
    const resultIndex = Math.min(MESSAGE_COUNT, assistantIndex + 1);
    const toolUseId = toolUseID(index + 1);
    messageToolRefRows.push([
      messageId(sourceIndex),
      messageId(assistantIndex),
      toolUseId,
      "tool_use",
      0
    ]);
    messageToolRefRows.push([
      messageId(resultIndex),
      messageId(assistantIndex),
      toolUseId,
      "tool_result",
      0
    ]);
  }
  await batchInsert(
    database,
    `INSERT INTO message_tool_refs (source_message_id, assistant_message_id, tool_use_id, block_type, cancelled)`,
    messageToolRefRows,
    50
  );
  const toolCallRows = [];
  for (let index = 1; index <= TOOL_CALL_COUNT; index++) {
    const assistantIndex = 2 + (index - 1) % 42 * 2;
    toolCallRows.push([
      toolUseID(index),
      `provider-${index}`,
      `tool_${index % 21}`,
      JSON.stringify({ path: `/tmp/file-${index}` }),
      "seed-executor",
      messageId(assistantIndex),
      isoAt(index),
      null,
      "completed",
      JSON.stringify({
        ok: true,
        run: { status: "done", result: text2(TOOL_CALL_RESULT_BYTES) }
      }),
      null,
      isoAt(index + 100)
    ]);
  }
  await batchInsert(
    database,
    `INSERT INTO tool_calls (id, provider_tool_use_id, tool_name, args, executor_id, message_id, issued_at, expires_at, state, result, progress, completed_at)`,
    toolCallRows,
    20
  );
  const executorToolRows = [];
  for (let index = 1; index <= EXECUTOR_TOOL_COUNT; index++) {
    const schema = JSON.stringify({
      name: `tool_${index}`,
      description: text2(EXECUTOR_TOOL_SCHEMA_BYTES),
      input_schema: { type: "object", properties: {} }
    });
    executorToolRows.push([
      "seed-executor",
      `tool_${index}`,
      schema,
      isoAt(index)
    ]);
  }
  await batchInsert(
    database,
    `INSERT INTO executor_tools (executor_id, tool_name, schema, updated_at)`,
    executorToolRows,
    42
  );
  const threadEventRows = [];
  for (let index = 1; index <= THREAD_EVENT_COUNT; index++) {
    threadEventRows.push([
      index,
      index % 3 === 0 ? "message_added" : "agent_state_changed",
      JSON.stringify({
        type: "seed_event",
        body: text2(THREAD_EVENT_PAYLOAD_BYTES)
      }),
      isoAt(index)
    ]);
  }
  await batchInsert(
    database,
    `INSERT INTO thread_events (seq, event_type, payload, created_at)`,
    threadEventRows,
    25
  );
  const messageAddedRows = [];
  for (let index = 1; index <= MESSAGE_COUNT; index++) {
    messageAddedRows.push([messageId(index), index]);
  }
  await batchInsert(
    database,
    `INSERT INTO message_added_events (message_id, seq)`,
    messageAddedRows,
    50
  );
  await database.execute(
    `INSERT INTO environment_snapshot (id, snapshot, updated_at) VALUES (1, ?, ?)`,
    JSON.stringify({ cwd: "/workspace", body: text2(3620) }),
    isoAt(0)
  );
  await database.execute(
    `INSERT INTO thread_settings_snapshot (id, settings, updated_at) VALUES (1, ?, ?)`,
    JSON.stringify({ maxTokens: 2e4, body: text2(55) }),
    isoAt(0)
  );
  await database.execute(
    `INSERT INTO e2b_sandbox (id, sandbox_id, restart_attempts, traffic_access_token, project_id, repository_url, additional_repositories, setup, created_at, updated_at)
			VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    "sandbox-seed",
    0,
    "token-seed",
    "project-seed",
    "https://example.invalid/repo.git",
    JSON.stringify([]),
    JSON.stringify({ commands: [] }),
    isoAt(0),
    isoAt(0)
  );
}
async function batchInsert(database, insertPrefix, rows, batchSize = 100) {
  if (rows.length === 0) {
    return;
  }
  const columnCount = rows[0]?.length ?? 0;
  if (columnCount === 0) {
    return;
  }
  const rowPlaceholder = `(${"?,".repeat(columnCount).slice(0, -1)})`;
  for (let index = 0; index < rows.length; index += batchSize) {
    const chunk = rows.slice(index, index + batchSize);
    const values = chunk.map(() => rowPlaceholder).join(",");
    const bindings = chunk.flat();
    await database.execute(`${insertPrefix} VALUES ${values}`, ...bindings);
  }
}
function messageId(index) {
  return `M-${String(index).padStart(22, "0")}`;
}
function toolUseID(index) {
  return `toolu_${String(index).padStart(22, "0")}`;
}
function safeId(value) {
  return value.replace(/[^a-zA-Z0-9_-]/g, "-").slice(0, 80);
}

// src/actors/testing/mock-agentic-loop.ts
import {
  actor as actor41
} from "rivetkit";
import { db as db7 } from "rivetkit/db";
var DEFAULT_SLEEP_GRACE_PERIOD_MS = 12e4;
var DEFAULT_ON_SLEEP_DELAY_MS = 0;
var debugSocketsByActorId = /* @__PURE__ */ new Map();
function sleep4(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, ms));
}
function positiveInteger(value, name) {
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${name} must be a positive integer`);
  }
  return value;
}
function stringValue(value, name) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`${name} must be a non-empty string`);
  }
  return value;
}
function typedRows(rows) {
  return rows;
}
function numberFromEnv(name, fallback) {
  const value = process.env[name];
  if (value === void 0 || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`${name} must be a finite non-negative number`);
  }
  return parsed;
}
function send3(websocket, payload2) {
  if (websocket.readyState !== 1) return;
  websocket.send(JSON.stringify(payload2));
}
function debugPayload(row, replayed) {
  return {
    type: "debugEvent",
    eventId: row.event_id,
    name: row.name,
    actorId: row.actor_id,
    connectionId: row.connection_id,
    requestId: row.request_id,
    details: JSON.parse(row.details_json),
    createdAt: row.created_at,
    replayed
  };
}
function publishDebugEvent(row) {
  const sockets = debugSocketsByActorId.get(row.actor_id);
  if (!sockets) return;
  for (const socket of sockets) {
    send3(socket, debugPayload(row, false));
  }
}
function addDebugSocket(actorId, websocket) {
  const sockets = debugSocketsByActorId.get(actorId) ?? /* @__PURE__ */ new Set();
  sockets.add(websocket);
  debugSocketsByActorId.set(actorId, sockets);
  return () => {
    sockets.delete(websocket);
    if (sockets.size === 0) {
      debugSocketsByActorId.delete(actorId);
    }
  };
}
async function recordDebugEvent(c, input) {
  const row = {
    event_id: crypto.randomUUID(),
    name: input.name,
    actor_id: c.actorId,
    connection_id: input.connectionId ?? null,
    request_id: input.requestId ?? null,
    details_json: JSON.stringify(input.details ?? {}),
    created_at: input.createdAt ?? Date.now()
  };
  try {
    await c.db.execute(
      "INSERT INTO mock_agentic_debug_events (event_id, name, actor_id, connection_id, request_id, details_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
      row.event_id,
      row.name,
      row.actor_id,
      row.connection_id,
      row.request_id,
      row.details_json,
      row.created_at
    );
    publishDebugEvent(row);
  } catch (error) {
    c.log.warn({
      msg: "mock agentic debug event failed",
      name: input.name,
      err: error instanceof Error ? error.message : String(error)
    });
  }
}
async function replayDebugEvents(database, websocket) {
  const rows = typedRows(
    await database.execute(`
			SELECT event_id, name, actor_id, connection_id, request_id, details_json, created_at
			FROM (
				SELECT event_id, name, actor_id, connection_id, request_id, details_json, created_at
				FROM mock_agentic_debug_events
				ORDER BY created_at DESC
				LIMIT 200
			)
			ORDER BY created_at ASC
		`)
  );
  for (const row of rows) {
    send3(websocket, debugPayload(row, true));
  }
}
function verifyEntryRows(rows, expectedSeconds) {
  const seen = /* @__PURE__ */ new Set();
  const indexes = rows.map((row) => row.idx).sort((a, b) => a - b);
  for (const idx of indexes) seen.add(idx);
  const missing = [];
  for (let idx = 1; idx <= expectedSeconds; idx += 1) {
    if (!seen.has(idx)) missing.push(idx);
  }
  const contiguous = rows.length === expectedSeconds && missing.length === 0 && indexes.every((idx, offset) => idx === offset + 1);
  return {
    expectedSeconds,
    count: rows.length,
    contiguous,
    missing,
    indexes,
    ok: contiguous
  };
}
function verifyAllRows(rows, expectedRequests) {
  const expectedByRequest = new Map(
    expectedRequests.map((request) => [request.requestId, request.seconds])
  );
  const rowsByRequest = /* @__PURE__ */ new Map();
  for (const row of rows) {
    const requestRows = rowsByRequest.get(row.request_id) ?? [];
    requestRows.push(row);
    rowsByRequest.set(row.request_id, requestRows);
  }
  const requests = expectedRequests.map((request) => {
    const result = verifyEntryRows(
      rowsByRequest.get(request.requestId) ?? [],
      request.seconds
    );
    return {
      requestId: request.requestId,
      ...result
    };
  });
  const unexpectedRequestIds = [...rowsByRequest.keys()].filter((requestId) => !expectedByRequest.has(requestId)).sort();
  const expectedTotalRows = expectedRequests.reduce(
    (total, request) => total + request.seconds,
    0
  );
  const ok = unexpectedRequestIds.length === 0 && rows.length === expectedTotalRows && requests.every((request) => request.ok);
  return {
    type: "verifiedAll",
    expectedRequests: expectedRequests.length,
    expectedTotalRows,
    totalRows: rows.length,
    rows,
    unexpectedRequestIds,
    requests,
    ok
  };
}
var mockAgenticLoop = actor41({
  options: {
    canHibernateWebSocket: false,
    sleepGracePeriod: DEFAULT_SLEEP_GRACE_PERIOD_MS
  },
  db: db7({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS mock_agentic_entries (
					request_id TEXT NOT NULL,
					idx INTEGER NOT NULL,
					created_at INTEGER NOT NULL,
					PRIMARY KEY (request_id, idx)
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_mock_agentic_entries_created_at ON mock_agentic_entries(created_at)"
      );
      await database.execute(`
				CREATE TABLE IF NOT EXISTS mock_agentic_sleep_state (
					id INTEGER PRIMARY KEY CHECK (id = 1),
					sleep_started_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS mock_agentic_debug_events (
					event_id TEXT PRIMARY KEY,
					name TEXT NOT NULL,
					actor_id TEXT NOT NULL,
					connection_id TEXT,
					request_id TEXT,
					details_json TEXT NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_mock_agentic_debug_events_created_at ON mock_agentic_debug_events(created_at)"
      );
    }
  }),
  async onWake(c) {
    await recordDebugEvent(c, {
      name: "onWake",
      details: {
        key: c.key,
        name: c.name
      }
    });
  },
  async onSleep(c) {
    const delayMs = numberFromEnv(
      "MOCK_AGENTIC_ON_SLEEP_DELAY_MS",
      DEFAULT_ON_SLEEP_DELAY_MS
    );
    const sleepStartedAt = Date.now();
    await recordDebugEvent(c, {
      name: "onSleepStart",
      createdAt: sleepStartedAt,
      details: {
        delayMs
      }
    });
    await c.db.execute(
      "INSERT OR REPLACE INTO mock_agentic_sleep_state (id, sleep_started_at) VALUES (1, ?)",
      sleepStartedAt
    );
    c.log.info({
      msg: "mock agentic loop onSleep delay",
      delayMs,
      sleepStartedAt
    });
    await sleep4(delayMs);
    await recordDebugEvent(c, {
      name: "onSleepEnd",
      details: {
        delayMs,
        sleepStartedAt,
        elapsedMs: Date.now() - sleepStartedAt
      }
    });
  },
  async onRequest(c, request) {
    const url = new URL(request.url);
    if (url.pathname === "/bypass" || url.pathname === "/request/bypass") {
      const [sleepState] = typedRows(
        await c.db.execute(
          "SELECT sleep_started_at FROM mock_agentic_sleep_state WHERE id = 1"
        )
      );
      return new Response(
        JSON.stringify({
          type: "bypass",
          transport: "http",
          sleepStarted: sleepState !== void 0,
          sleepStartedAt: sleepState?.sleep_started_at ?? null,
          timestamp: Date.now()
        }),
        {
          headers: {
            "content-type": "application/json"
          }
        }
      );
    }
    return new Response("not found", { status: 404 });
  },
  onWebSocket(c, websocket) {
    const connectionId = crypto.randomUUID();
    let activeInference;
    const removeDebugSocket = addDebugSocket(c.actorId, websocket);
    send3(websocket, {
      type: "hello",
      connectionId,
      timestamp: Date.now()
    });
    void (async () => {
      try {
        await replayDebugEvents(c.db, websocket);
      } catch (error) {
        c.log.warn({
          msg: "mock agentic debug replay failed",
          err: error instanceof Error ? error.message : String(error)
        });
      }
      await recordDebugEvent(c, {
        name: "webSocketOpen",
        connectionId
      });
    })();
    const verify = async (requestId, expectedSeconds) => {
      const rows = typedRows(
        await c.db.execute(
          "SELECT request_id, idx, created_at FROM mock_agentic_entries WHERE request_id = ? ORDER BY idx ASC",
          requestId
        )
      );
      return {
        type: "verified",
        requestId,
        ...verifyEntryRows(rows, expectedSeconds)
      };
    };
    const sleepStatus = async () => {
      const [sleepState] = typedRows(
        await c.db.execute(
          "SELECT sleep_started_at FROM mock_agentic_sleep_state WHERE id = 1"
        )
      );
      return {
        sleepStarted: sleepState !== void 0,
        sleepStartedAt: sleepState?.sleep_started_at ?? null
      };
    };
    const runInference = async (requestId, seconds) => {
      send3(websocket, {
        type: "started",
        requestId,
        seconds,
        timestamp: Date.now()
      });
      await c.db.execute(
        "DELETE FROM mock_agentic_entries WHERE request_id = ?",
        requestId
      );
      for (let idx = 1; idx <= seconds; idx += 1) {
        await sleep4(1e3);
        const createdAt = Date.now();
        await c.db.execute(
          "INSERT INTO mock_agentic_entries (request_id, idx, created_at) VALUES (?, ?, ?)",
          requestId,
          idx,
          createdAt
        );
        send3(websocket, {
          type: "progress",
          requestId,
          idx,
          seconds,
          createdAt
        });
      }
      const verification = await verify(requestId, seconds);
      send3(websocket, {
        type: "done",
        requestId,
        seconds,
        timestamp: Date.now(),
        verification
      });
    };
    websocket.addEventListener(
      "message",
      async (event19) => {
        try {
          if (typeof event19.data !== "string") {
            throw new Error("message data must be a JSON string");
          }
          const message = JSON.parse(event19.data);
          const type = stringValue(message.type, "type");
          if (type === "history") {
            const rows = typedRows(
              await c.db.execute(
                "SELECT request_id, idx, created_at FROM mock_agentic_entries ORDER BY created_at ASC, request_id ASC, idx ASC"
              )
            );
            const [count] = typedRows(
              await c.db.execute(
                "SELECT COUNT(*) AS count FROM mock_agentic_entries"
              )
            );
            send3(websocket, {
              type: "history",
              totalRows: count?.count ?? rows.length,
              entries: rows,
              timestamp: Date.now()
            });
            return;
          }
          if (type === "ping") {
            send3(websocket, {
              type: "pong",
              probeId: stringValue(message.probeId, "probeId"),
              ...await sleepStatus(),
              timestamp: Date.now()
            });
            return;
          }
          if (type === "verify") {
            const requestId = stringValue(
              message.requestId,
              "requestId"
            );
            const expectedSeconds = positiveInteger(
              message.expectedSeconds,
              "expectedSeconds"
            );
            send3(
              websocket,
              await verify(requestId, expectedSeconds)
            );
            return;
          }
          if (type === "infer") {
            const requestId = stringValue(
              message.requestId,
              "requestId"
            );
            const seconds = positiveInteger(
              message.seconds,
              "seconds"
            );
            await recordDebugEvent(c, {
              name: "inferenceRequested",
              connectionId,
              requestId,
              details: {
                seconds
              }
            });
            const previousInference = activeInference;
            const inference = (async () => {
              await previousInference?.catch(() => void 0);
              await runInference(requestId, seconds);
            })();
            activeInference = inference;
            await c.keepAwake(inference);
            if (activeInference === inference) {
              activeInference = void 0;
            }
            return;
          }
          throw new Error(`unknown message type: ${type}`);
        } catch (error) {
          send3(websocket, {
            type: "error",
            message: error instanceof Error ? error.message : "unknown websocket error",
            timestamp: Date.now()
          });
        }
      }
    );
    websocket.addEventListener("close", async () => {
      removeDebugSocket();
      await recordDebugEvent(c, {
        name: "webSocketClose",
        connectionId
      });
    });
  },
  actions: {
    verify: async (c, requestId, expectedSeconds) => {
      const rows = typedRows(
        await c.db.execute(
          "SELECT request_id, idx, created_at FROM mock_agentic_entries WHERE request_id = ? ORDER BY idx ASC",
          requestId
        )
      );
      return {
        requestId,
        expectedSeconds,
        count: rows.length,
        indexes: rows.map((row) => row.idx)
      };
    },
    verifyAll: async (c, expectedRequests) => {
      if (!Array.isArray(expectedRequests)) {
        throw new Error("expectedRequests must be an array");
      }
      for (const request of expectedRequests) {
        stringValue(request.requestId, "requestId");
        positiveInteger(request.seconds, "seconds");
      }
      const rows = typedRows(
        await c.db.execute(
          "SELECT request_id, idx, created_at FROM mock_agentic_entries ORDER BY request_id ASC, idx ASC"
        )
      );
      return verifyAllRows(rows, expectedRequests);
    }
  }
});

// src/actors/testing/raw-sqlite-fuzzer.ts
import { actor as actor42 } from "rivetkit";
import { db as db8 } from "rivetkit/db";
var ACCOUNT_COUNT = 8;
var ACCOUNT_INITIAL_BALANCE = 1e5;
var DEFAULT_KEY_SPACE = 64;
var DEFAULT_MAX_PAYLOAD_BYTES = 8 * 1024;
var DEFAULT_GROWTH_TARGET_BYTES = 1024 * 1024;
var LARGE_WRITE_CHUNK_BYTES = 96 * 1024;
var PAGE_BOUNDARY_SIZES = [
  1,
  4095,
  4096,
  4097,
  8191,
  8192,
  8193,
  32768,
  65535,
  65536,
  98304,
  131072
];
function hashSeed(input) {
  let hash = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}
function makeRng(seed) {
  let state = hashSeed(seed) || 2654435769;
  return () => {
    state = state + 1831565813 >>> 0;
    let t = state;
    t = Math.imul(t ^ t >>> 15, t | 1);
    t ^= t + Math.imul(t ^ t >>> 7, t | 61);
    return ((t ^ t >>> 14) >>> 0) / 4294967296;
  };
}
function intBetween(rng, min, max) {
  return min + Math.floor(rng() * (max - min + 1));
}
function checksum(input) {
  let hash = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}
function payloadFor(seed, phase, index, bytes) {
  const prefix = `${seed}:${phase}:${index}:`;
  if (bytes <= prefix.length) return prefix.slice(0, bytes);
  return prefix + "x".repeat(bytes - prefix.length);
}
async function queryOne2(database, sql, ...args) {
  const rows = await database.execute(sql, ...args);
  return rows[0];
}
async function transaction(database, fn) {
  await database.execute("BEGIN");
  try {
    const result = await fn();
    await database.execute("COMMIT");
    return result;
  } catch (err) {
    await database.execute("ROLLBACK").catch(() => void 0);
    throw err;
  }
}
async function recordProbe(database, phase, scenario, name, expected, actual, mismatch) {
  await database.execute(
    `INSERT INTO fuzz_probe_results (
			phase, scenario, name, expected, actual, mismatch, created_at
		) VALUES (?, ?, ?, ?, ?, ?, ?)`,
    phase,
    scenario,
    name,
    String(expected),
    String(actual),
    mismatch ? 1 : 0,
    Date.now()
  );
}
function firstColumn(row) {
  if (!row || typeof row !== "object") return void 0;
  const values = Object.values(row);
  return values[0];
}
async function ensureAccounts(database) {
  await database.execute("BEGIN");
  try {
    for (let i = 0; i < ACCOUNT_COUNT; i += 1) {
      await database.execute(
        "INSERT OR IGNORE INTO fuzz_accounts (id, balance) VALUES (?, ?)",
        `acct-${i}`,
        ACCOUNT_INITIAL_BALANCE
      );
    }
    await database.execute("COMMIT");
  } catch (err) {
    await database.execute("ROLLBACK").catch(() => void 0);
    throw err;
  }
}
async function recordItemEvent(database, phase, localIndex, kind, itemKey, present, value, version, updateCount, payload2, applied) {
  await database.execute(
    `INSERT INTO fuzz_item_events (
			phase, local_index, kind, item_key, present, value, version,
			update_count, payload_checksum, payload_bytes, applied, created_at
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    phase,
    localIndex,
    kind,
    itemKey,
    present ? 1 : 0,
    value,
    version,
    updateCount,
    checksum(payload2),
    payload2.length,
    applied ? 1 : 0,
    Date.now()
  );
}
async function upsertLiveItem(database, row, payload2) {
  await database.execute(
    `INSERT INTO fuzz_items (
			item_key, value, version, update_count, payload, payload_checksum,
			payload_bytes, updated_at
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
		ON CONFLICT(item_key) DO UPDATE SET
			value = excluded.value,
			version = excluded.version,
			update_count = excluded.update_count,
			payload = excluded.payload,
			payload_checksum = excluded.payload_checksum,
			payload_bytes = excluded.payload_bytes,
			updated_at = excluded.updated_at`,
    row.item_key,
    row.value,
    row.version,
    row.update_count,
    payload2,
    row.payload_checksum,
    row.payload_bytes,
    Date.now()
  );
}
async function applyItemOperation(database, opts) {
  let current;
  try {
    current = await queryOne2(
      database,
      "SELECT item_key, value, version, update_count, payload, payload_checksum, payload_bytes FROM fuzz_items WHERE item_key = ?",
      opts.itemKey
    );
  } catch (error) {
    throw new Error(
      `item operation select failed for kind ${opts.kind} key ${JSON.stringify(opts.itemKey)} payloadBytes ${opts.payloadBytes}`,
      { cause: error }
    );
  }
  const payload2 = payloadFor(
    opts.seed,
    opts.phase,
    opts.localIndex,
    opts.payloadBytes
  );
  const nextVersion = (current?.version ?? 0) + 1;
  const nextUpdateCount = (current?.update_count ?? 0) + 1;
  const nextValue = `${opts.kind}:${opts.phase}:${opts.localIndex}:${nextVersion}`;
  if (opts.kind === "delete") {
    try {
      await recordItemEvent(
        database,
        opts.phase,
        opts.localIndex,
        opts.kind,
        opts.itemKey,
        false,
        null,
        nextVersion,
        nextUpdateCount,
        "",
        current !== void 0
      );
    } catch (error) {
      throw new Error(
        `item operation event insert failed for delete key ${JSON.stringify(opts.itemKey)}`,
        {
          cause: error
        }
      );
    }
    try {
      await database.execute(
        "DELETE FROM fuzz_items WHERE item_key = ?",
        opts.itemKey
      );
    } catch (error) {
      throw new Error(
        `item operation delete failed for key ${JSON.stringify(opts.itemKey)}`,
        {
          cause: error
        }
      );
    }
    return;
  }
  if (opts.kind === "insert" && current) {
    try {
      await recordItemEvent(
        database,
        opts.phase,
        opts.localIndex,
        opts.kind,
        opts.itemKey,
        true,
        current.value,
        current.version,
        current.update_count,
        current.payload ?? "",
        false
      );
    } catch (error) {
      throw new Error(
        `item operation event insert failed for noop insert key ${JSON.stringify(opts.itemKey)}`,
        { cause: error }
      );
    }
    return;
  }
  const row = {
    item_key: opts.itemKey,
    value: nextValue,
    version: nextVersion,
    update_count: nextUpdateCount,
    payload_checksum: checksum(payload2),
    payload_bytes: payload2.length
  };
  try {
    await recordItemEvent(
      database,
      opts.phase,
      opts.localIndex,
      opts.kind,
      opts.itemKey,
      true,
      row.value,
      row.version,
      row.update_count,
      payload2,
      true
    );
  } catch (error) {
    throw new Error(
      `item operation event insert failed for kind ${opts.kind} key ${JSON.stringify(opts.itemKey)} payloadBytes ${opts.payloadBytes}`,
      { cause: error }
    );
  }
  try {
    await upsertLiveItem(database, row, payload2);
  } catch (error) {
    throw new Error(
      `item operation live-row upsert failed for kind ${opts.kind} key ${JSON.stringify(opts.itemKey)} payloadBytes ${opts.payloadBytes}`,
      { cause: error }
    );
  }
}
async function applyHotUpdates(database, opts) {
  for (let i = 0; i < opts.updates; i += 1) {
    try {
      await applyItemOperation(database, {
        seed: opts.seed,
        phase: opts.phase,
        localIndex: opts.localIndex * 1e3 + i,
        kind: "update",
        itemKey: opts.itemKey,
        payloadBytes: opts.payloadBytes
      });
    } catch (error) {
      throw new Error(
        `hot update failed for ${opts.itemKey} at sub-update ${i + 1}/${opts.updates} with payloadBytes ${opts.payloadBytes}`,
        { cause: error }
      );
    }
  }
}
async function applyTransfer(database, opts) {
  await transaction(database, async () => {
    const before = await queryOne2(
      database,
      "SELECT COALESCE(SUM(balance), 0) AS total FROM fuzz_accounts"
    );
    await database.execute(
      "UPDATE fuzz_accounts SET balance = balance - ? WHERE id = ?",
      opts.amount,
      opts.fromAccount
    );
    await database.execute(
      "UPDATE fuzz_accounts SET balance = balance + ? WHERE id = ?",
      opts.amount,
      opts.toAccount
    );
    const after = await queryOne2(
      database,
      "SELECT COALESCE(SUM(balance), 0) AS total FROM fuzz_accounts"
    );
    await database.execute(
      `INSERT INTO fuzz_transfer_events (
				phase, local_index, from_account, to_account, amount,
				balance_sum_before, balance_sum_after, created_at
			) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
      opts.phase,
      opts.localIndex,
      opts.fromAccount,
      opts.toAccount,
      opts.amount,
      before?.total ?? 0,
      after?.total ?? 0,
      Date.now()
    );
  });
}
async function applyEdgePayloads(database, opts) {
  const writeEdgePayload = async (id, kind, payload2, sizeLabel) => {
    const payloadChecksum = checksum(payload2);
    const payloadBytes = payload2.length;
    try {
      await database.execute("BEGIN");
    } catch (error) {
      throw new Error(`edge payload begin failed for ${sizeLabel}`, {
        cause: error
      });
    }
    try {
      try {
        await database.execute(
          `INSERT INTO fuzz_edge_payloads (
						id, kind, payload, payload_checksum, payload_bytes, updated_at
					) VALUES (?, ?, ?, ?, ?, ?)
					ON CONFLICT(id) DO UPDATE SET
						kind = excluded.kind,
						payload = excluded.payload,
						payload_checksum = excluded.payload_checksum,
						payload_bytes = excluded.payload_bytes,
						updated_at = excluded.updated_at`,
          id,
          kind,
          payload2,
          payloadChecksum,
          payloadBytes,
          Date.now()
        );
      } catch (error) {
        throw new Error(
          `edge payload row upsert failed for ${sizeLabel}`,
          {
            cause: error
          }
        );
      }
      try {
        await database.execute(
          `INSERT INTO fuzz_edge_expectations (
						id, present, payload_checksum, payload_bytes
					) VALUES (?, 1, ?, ?)
					ON CONFLICT(id) DO UPDATE SET
						present = excluded.present,
						payload_checksum = excluded.payload_checksum,
						payload_bytes = excluded.payload_bytes`,
          id,
          payloadChecksum,
          payloadBytes
        );
      } catch (error) {
        throw new Error(
          `edge payload expectation upsert failed for ${sizeLabel}`,
          {
            cause: error
          }
        );
      }
      try {
        await database.execute("COMMIT");
      } catch (error) {
        throw new Error(`edge payload commit failed for ${sizeLabel}`, {
          cause: error
        });
      }
    } catch (error) {
      await database.execute("ROLLBACK").catch(() => void 0);
      throw error;
    }
  };
  const sizes = PAGE_BOUNDARY_SIZES.filter(
    (size) => size <= opts.maxPayloadBytes
  );
  if (!sizes.includes(opts.maxPayloadBytes)) sizes.push(opts.maxPayloadBytes);
  let ops = 0;
  for (const size of sizes) {
    const id = `edge-${opts.phase}-${size}`;
    const payload2 = payloadFor(opts.seed, opts.phase, size, size);
    try {
      await writeEdgePayload(id, "boundary", payload2, `size ${size}`);
    } catch (error) {
      throw new Error(`edge payload write failed for size ${size}`, {
        cause: error
      });
    }
    ops += 1;
  }
  const unicodePayload = `escaped-nul:\\0 unicode:\u2603\uFE0F phase:${opts.phase} seed:${opts.seed}`;
  const unicodeId = `edge-${opts.phase}-unicode-nul`;
  try {
    await writeEdgePayload(
      unicodeId,
      "unicode-nul",
      unicodePayload,
      "unicode escaped-nul payload"
    );
  } catch (error) {
    throw new Error(
      "edge payload write failed for unicode escaped-nul payload",
      {
        cause: error
      }
    );
  }
  return ops + 1;
}
async function applyActualNulPayload(database, opts) {
  const payload2 = `actual-nul:\0 phase:${opts.phase} seed:${opts.seed}`;
  const id = `actual-nul-${opts.phase}`;
  await transaction(database, async () => {
    await database.execute(
      `INSERT INTO fuzz_edge_payloads (
				id, kind, payload, payload_checksum, payload_bytes, updated_at
			) VALUES (?, 'actual-nul', ?, ?, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				payload = excluded.payload,
				payload_checksum = excluded.payload_checksum,
				payload_bytes = excluded.payload_bytes,
				updated_at = excluded.updated_at`,
      id,
      payload2,
      checksum(payload2),
      payload2.length,
      Date.now()
    );
    await database.execute(
      `INSERT INTO fuzz_edge_expectations (
				id, present, payload_checksum, payload_bytes
			) VALUES (?, 1, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				present = 1,
				payload_checksum = excluded.payload_checksum,
				payload_bytes = excluded.payload_bytes`,
      id,
      checksum(payload2),
      payload2.length
    );
  });
  return 1;
}
async function applyFragmentationChurn(database, opts) {
  const rows = Math.max(12, Math.floor(opts.iterations / 2));
  let ops = 0;
  for (let i = 0; i < rows; i += 1) {
    const size = intBetween(
      opts.rng,
      32,
      Math.max(32, opts.maxPayloadBytes)
    );
    const id = `frag-${opts.phase}-${i}`;
    const payload2 = payloadFor(opts.seed, opts.phase, 1e4 + i, size);
    try {
      await database.execute(
        `INSERT INTO fuzz_edge_payloads (
					id, kind, payload, payload_checksum, payload_bytes, updated_at
				) VALUES (?, 'fragment', ?, ?, ?, ?)
				ON CONFLICT(id) DO UPDATE SET
					payload = excluded.payload,
					payload_checksum = excluded.payload_checksum,
					payload_bytes = excluded.payload_bytes,
					updated_at = excluded.updated_at`,
        id,
        payload2,
        checksum(payload2),
        payload2.length,
        Date.now()
      );
    } catch (error) {
      throw new Error(
        `fragmentation payload upsert failed for ${id} at size ${size}`,
        {
          cause: error
        }
      );
    }
    try {
      await database.execute(
        `INSERT INTO fuzz_edge_expectations (
					id, present, payload_checksum, payload_bytes
				) VALUES (?, 1, ?, ?)
				ON CONFLICT(id) DO UPDATE SET
					present = 1,
					payload_checksum = excluded.payload_checksum,
					payload_bytes = excluded.payload_bytes`,
        id,
        checksum(payload2),
        payload2.length
      );
    } catch (error) {
      throw new Error(
        `fragmentation expectation upsert failed for ${id} at size ${size}`,
        {
          cause: error
        }
      );
    }
    ops += 1;
  }
  for (let i = 0; i < rows; i += 3) {
    const id = `frag-${opts.phase}-${i}`;
    try {
      await database.execute(
        "DELETE FROM fuzz_edge_payloads WHERE id = ?",
        id
      );
    } catch (error) {
      throw new Error(`fragmentation delete failed for ${id}`, {
        cause: error
      });
    }
    try {
      await database.execute(
        `INSERT INTO fuzz_edge_expectations (
					id, present, payload_checksum, payload_bytes
				) VALUES (?, 0, 0, 0)
				ON CONFLICT(id) DO UPDATE SET
					present = 0,
					payload_checksum = 0,
					payload_bytes = 0`,
        id
      );
    } catch (error) {
      throw new Error(
        `fragmentation tombstone expectation failed for ${id}`,
        {
          cause: error
        }
      );
    }
    ops += 1;
  }
  for (let i = 1; i < rows; i += 4) {
    const size = intBetween(opts.rng, 1, Math.max(1, opts.maxPayloadBytes));
    const id = `frag-${opts.phase}-${i}`;
    const payload2 = payloadFor(opts.seed, opts.phase, 2e4 + i, size);
    try {
      await database.execute(
        `UPDATE fuzz_edge_payloads
				SET payload = ?, payload_checksum = ?, payload_bytes = ?, updated_at = ?
				WHERE id = ?`,
        payload2,
        checksum(payload2),
        payload2.length,
        Date.now(),
        id
      );
    } catch (error) {
      throw new Error(
        `fragmentation payload rewrite failed for ${id} at size ${size}`,
        {
          cause: error
        }
      );
    }
    try {
      await database.execute(
        `UPDATE fuzz_edge_expectations
				SET payload_checksum = ?, payload_bytes = ?
				WHERE id = ? AND present = 1`,
        checksum(payload2),
        payload2.length,
        id
      );
    } catch (error) {
      throw new Error(
        `fragmentation expectation rewrite failed for ${id} at size ${size}`,
        {
          cause: error
        }
      );
    }
    ops += 1;
  }
  if (opts.phase % 2 === 1) {
    try {
      await database.execute("VACUUM");
    } catch (error) {
      throw new Error(
        `fragmentation vacuum failed for phase ${opts.phase}`,
        {
          cause: error
        }
      );
    }
    ops += 1;
  }
  return ops;
}
async function applySchemaChurn(database, phase) {
  const table = `fuzz_schema_phase_${phase}`;
  const index = `idx_fuzz_schema_phase_${phase}_name`;
  const view = `view_fuzz_schema_phase_${phase}`;
  const dropIndex = `idx_fuzz_schema_drop_probe_${phase}`;
  await database.execute(
    `CREATE TABLE IF NOT EXISTS ${table} (
			id INTEGER PRIMARY KEY,
			name TEXT NOT NULL UNIQUE,
			value INTEGER NOT NULL DEFAULT 0,
			extra TEXT
		)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS ${index} ON ${table}(name, value)`
  );
  try {
    await database.execute(
      `ALTER TABLE ${table} ADD COLUMN altered_${phase} TEXT DEFAULT 'altered'`
    );
  } catch {
    const column = await queryOne2(
      database,
      `SELECT COUNT(*) AS count FROM pragma_table_info('${table}') WHERE name = ?`,
      `altered_${phase}`
    );
    if ((column?.count ?? 0) !== 1)
      throw new Error(`failed to add altered_${phase}`);
  }
  await database.execute(
    `CREATE VIEW IF NOT EXISTS ${view} AS SELECT id, name, value FROM ${table}`
  );
  await database.execute(
    `INSERT INTO ${table} (name, value, extra)
		VALUES (?, ?, ?)
		ON CONFLICT(name) DO UPDATE SET
			value = excluded.value,
			extra = excluded.extra`,
    `schema-${phase}`,
    phase,
    `extra-${phase}`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS fuzz_without_rowid (
			id TEXT PRIMARY KEY,
			value INTEGER NOT NULL
		) WITHOUT ROWID`
  );
  await database.execute(`
		CREATE TRIGGER IF NOT EXISTS trg_fuzz_edge_payload_update
		AFTER UPDATE ON fuzz_edge_payloads
		BEGIN
			INSERT INTO fuzz_trigger_audit (
				payload_id, old_checksum, new_checksum, created_at
			) VALUES (
				new.id, old.payload_checksum, new.payload_checksum, strftime('%s', 'now') * 1000
			);
		END
	`);
  await database.execute(
    `INSERT INTO fuzz_without_rowid (id, value)
		VALUES (?, ?)
		ON CONFLICT(id) DO UPDATE SET value = excluded.value`,
    `phase-${phase}`,
    phase
  );
  for (const [name, type] of [
    [table, "table"],
    [index, "index"],
    [view, "view"],
    ["trg_fuzz_edge_payload_update", "trigger"],
    ["fuzz_without_rowid", "table"]
  ]) {
    await database.execute(
      `INSERT INTO fuzz_schema_registry (name, type)
			VALUES (?, ?)
			ON CONFLICT(name) DO UPDATE SET type = excluded.type`,
      name,
      type
    );
  }
  await database.execute(
    "CREATE TEMP TABLE IF NOT EXISTS fuzz_temp_probe (id INTEGER PRIMARY KEY, value TEXT)"
  );
  await database.execute(
    "INSERT INTO fuzz_temp_probe (value) VALUES (?)",
    `temp-${phase}`
  );
  await database.execute("DROP TABLE fuzz_temp_probe");
  await database.execute(
    `CREATE INDEX IF NOT EXISTS ${dropIndex} ON fuzz_schema_registry(type)`
  );
  await database.execute(`DROP INDEX IF EXISTS ${dropIndex}`);
  const dropped = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'index' AND name = ?",
    dropIndex
  );
  await recordProbe(
    database,
    phase,
    "schema",
    "drop-index",
    0,
    dropped?.count ?? -1,
    (dropped?.count ?? -1) !== 0
  );
  return 13;
}
async function applyIndexProbe(database, opts) {
  const rows = Math.max(20, opts.iterations);
  await transaction(database, async () => {
    for (let i = 0; i < rows; i += 1) {
      const tenant = `tenant-${intBetween(opts.rng, 0, 5)}`;
      const bucket = intBetween(opts.rng, 0, 12);
      const score = intBetween(opts.rng, -500, 500);
      const label = `${opts.seed}:${opts.phase}:${i}`;
      await database.execute(
        `INSERT INTO fuzz_indexed (tenant, bucket, score, label, payload)
				VALUES (?, ?, ?, ?, ?)`,
        tenant,
        bucket,
        score,
        label,
        payloadFor(
          opts.seed,
          opts.phase,
          3e4 + i,
          intBetween(opts.rng, 8, 256)
        )
      );
    }
  });
  return rows;
}
async function applyPreparedChurn(database, opts) {
  const rows = Math.max(32, opts.iterations);
  for (let i = 0; i < rows; i += 1) {
    const id = `prep-${opts.phase}-${i}`;
    const payload2 = payloadFor(
      opts.seed,
      opts.phase,
      7e4 + i,
      Math.min(opts.maxPayloadBytes, 64 + i % 257)
    );
    await database.execute(
      `INSERT INTO fuzz_prepared_churn (id, value, payload, payload_checksum)
			VALUES (?, ?, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				value = excluded.value,
				payload = excluded.payload,
				payload_checksum = excluded.payload_checksum
			/* unique-prepared-${opts.phase}-${i} */`,
      id,
      i,
      payload2,
      checksum(payload2)
    );
    await database.execute(
      `INSERT INTO fuzz_prepared_expectations (id, value, payload_checksum)
			VALUES (?, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				value = excluded.value,
				payload_checksum = excluded.payload_checksum`,
      id,
      i,
      checksum(payload2)
    );
  }
  const repeatedId = `prep-repeat-${opts.phase}`;
  await database.execute(
    `INSERT INTO fuzz_prepared_churn (id, value, payload, payload_checksum)
		VALUES (?, 0, '', 0)
		ON CONFLICT(id) DO UPDATE SET value = 0, payload = '', payload_checksum = 0`,
    repeatedId
  );
  for (let i = 0; i < rows; i += 1) {
    const payload2 = payloadFor(
      opts.seed,
      opts.phase,
      8e4 + i,
      Math.min(512, opts.maxPayloadBytes)
    );
    await database.execute(
      `UPDATE fuzz_prepared_churn
			SET value = value + ?, payload = ?, payload_checksum = ?
			WHERE id = ?`,
      1,
      payload2,
      checksum(payload2),
      repeatedId
    );
  }
  const finalPayload = payloadFor(
    opts.seed,
    opts.phase,
    8e4 + rows - 1,
    Math.min(512, opts.maxPayloadBytes)
  );
  await database.execute(
    `INSERT INTO fuzz_prepared_expectations (id, value, payload_checksum)
		VALUES (?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			value = excluded.value,
			payload_checksum = excluded.payload_checksum`,
    repeatedId,
    rows,
    checksum(finalPayload)
  );
  return rows * 2 + 1;
}
async function applyReadWriteProbe(database, opts) {
  if ((await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_indexed"
  ))?.count === 0) {
    await applyIndexProbe(database, opts);
  }
  const read = database.execute(
    `SELECT
			COUNT(*) AS joined_rows,
			COALESCE(SUM(a.score + b.score), 0) AS score_sum
		FROM fuzz_indexed a
		JOIN fuzz_indexed b ON b.bucket = a.bucket
		WHERE a.tenant <= 'tenant-3'`
  );
  const write = applyIndexProbe(database, {
    seed: opts.seed,
    phase: opts.phase,
    rng: opts.rng,
    iterations: Math.max(10, Math.floor(opts.iterations / 2))
  });
  const [readRows, writeOps] = await Promise.all([read, write]);
  const row = readRows[0];
  const joinedRows = Number(row?.joined_rows ?? -1);
  const scoreSum = Number(row?.score_sum ?? Number.NaN);
  await recordProbe(
    database,
    opts.phase,
    "readwrite",
    "long-read-while-write",
    "nonnegative-finite",
    `${joinedRows}:${scoreSum}`,
    joinedRows < 0 || !Number.isFinite(scoreSum)
  );
  return writeOps + 1;
}
async function applyBoundaryKeys(database, opts) {
  const keys = [
    "",
    " ",
    `long-${"k".repeat(2048)}`,
    "slash/key",
    "comma,key",
    "percent%key",
    "CaseKey",
    "casekey"
  ];
  let ops = 0;
  for (const [index, key] of keys.entries()) {
    try {
      await applyItemOperation(database, {
        seed: opts.seed,
        phase: opts.phase,
        localIndex: 9e4 + index,
        kind: "upsert",
        itemKey: key,
        payloadBytes: Math.min(opts.maxPayloadBytes, 128 + index)
      });
    } catch (error) {
      throw new Error(
        `boundary key write failed for literal key ${JSON.stringify(key)} at index ${index}`,
        { cause: error }
      );
    }
    ops += 1;
  }
  for (let i = 0; i < 128; i += 1) {
    const itemKey = `seq-${opts.phase}-${i.toString().padStart(4, "0")}`;
    try {
      await applyItemOperation(database, {
        seed: opts.seed,
        phase: opts.phase,
        localIndex: 91e3 + i,
        kind: i % 4 === 0 ? "delete" : "upsert",
        itemKey,
        payloadBytes: Math.min(opts.maxPayloadBytes, 32 + i % 97)
      });
    } catch (error) {
      throw new Error(
        `boundary key write failed for sequential key ${JSON.stringify(itemKey)} at index ${i}`,
        { cause: error }
      );
    }
    ops += 1;
  }
  await recordProbe(
    database,
    opts.phase,
    "boundary-keys",
    "keys-written",
    136,
    ops,
    ops !== 136
  );
  return ops;
}
async function applyGrowthProbe(database, opts) {
  const chunkBytes = Math.max(
    1,
    Math.min(LARGE_WRITE_CHUNK_BYTES, opts.maxPayloadBytes)
  );
  const rows = Math.max(1, Math.ceil(opts.growthTargetBytes / chunkBytes));
  let written = 0;
  for (let i = 0; i < rows; i += 1) {
    const size = Math.min(chunkBytes, opts.growthTargetBytes - written);
    const id = `growth-${opts.phase}-${opts.growthTargetBytes}-${i}`;
    const payload2 = payloadFor(opts.seed, opts.phase, 1e5 + i, size);
    await database.execute(
      `INSERT INTO fuzz_edge_payloads (
				id, kind, payload, payload_checksum, payload_bytes, updated_at
			) VALUES (?, 'growth', ?, ?, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				payload = excluded.payload,
				payload_checksum = excluded.payload_checksum,
				payload_bytes = excluded.payload_bytes,
				updated_at = excluded.updated_at`,
      id,
      payload2,
      checksum(payload2),
      payload2.length,
      Date.now()
    );
    await database.execute(
      `INSERT INTO fuzz_edge_expectations (
				id, present, payload_checksum, payload_bytes
			) VALUES (?, 1, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				present = 1,
				payload_checksum = excluded.payload_checksum,
				payload_bytes = excluded.payload_bytes`,
      id,
      checksum(payload2),
      payload2.length
    );
    written += size;
  }
  await recordProbe(
    database,
    opts.phase,
    "growth",
    "target-bytes-written",
    opts.growthTargetBytes,
    written,
    written !== opts.growthTargetBytes
  );
  return rows;
}
async function applyTruncateRecreateProbe(database, opts) {
  const id = `truncate-${opts.phase}`;
  const largeSize = Math.max(1, Math.min(opts.maxPayloadBytes, 131072));
  const largePayload = payloadFor(opts.seed, opts.phase, 11e4, largeSize);
  const tinyPayload = payloadFor(opts.seed, opts.phase, 110001, 1);
  const recreatedPayload = payloadFor(
    opts.seed,
    opts.phase,
    110002,
    Math.min(4096, largeSize)
  );
  await database.execute(
    `INSERT INTO fuzz_edge_payloads (
			id, kind, payload, payload_checksum, payload_bytes, updated_at
		) VALUES (?, 'truncate', ?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			payload = excluded.payload,
			payload_checksum = excluded.payload_checksum,
			payload_bytes = excluded.payload_bytes,
			updated_at = excluded.updated_at`,
    id,
    largePayload,
    checksum(largePayload),
    largePayload.length,
    Date.now()
  );
  await database.execute(
    "UPDATE fuzz_edge_payloads SET payload = ?, payload_checksum = ?, payload_bytes = ?, updated_at = ? WHERE id = ?",
    tinyPayload,
    checksum(tinyPayload),
    tinyPayload.length,
    Date.now(),
    id
  );
  await database.execute("DELETE FROM fuzz_edge_payloads WHERE id = ?", id);
  await database.execute("VACUUM");
  await database.execute(
    `INSERT INTO fuzz_edge_payloads (
			id, kind, payload, payload_checksum, payload_bytes, updated_at
		) VALUES (?, 'truncate', ?, ?, ?, ?)`,
    id,
    recreatedPayload,
    checksum(recreatedPayload),
    recreatedPayload.length,
    Date.now()
  );
  await database.execute(
    `INSERT INTO fuzz_edge_expectations (
			id, present, payload_checksum, payload_bytes
		) VALUES (?, 1, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			present = 1,
			payload_checksum = excluded.payload_checksum,
			payload_bytes = excluded.payload_bytes`,
    id,
    checksum(recreatedPayload),
    recreatedPayload.length
  );
  return 5;
}
async function updateShadowChecksums(database, phase) {
  const item = await queryOne2(
    database,
    `SELECT COUNT(*) AS rows, COALESCE(SUM(payload_checksum + version + update_count), 0) AS value
		FROM fuzz_items`
  );
  const edge = await queryOne2(
    database,
    `SELECT COUNT(*) AS rows, COALESCE(SUM(payload_checksum + payload_bytes), 0) AS value
		FROM fuzz_edge_payloads`
  );
  await transaction(database, async () => {
    for (const [name, rows, value] of [
      ["items", item?.rows ?? 0, item?.value ?? 0],
      ["edge", edge?.rows ?? 0, edge?.value ?? 0]
    ]) {
      await database.execute(
        `INSERT INTO fuzz_shadow_checksums (name, value, row_count)
				VALUES (?, ?, ?)
				ON CONFLICT(name) DO UPDATE SET
					value = excluded.value,
					row_count = excluded.row_count`,
        name,
        value,
        rows
      );
    }
  });
  await recordProbe(database, phase, "shadow", "shadow-updated", 2, 2, false);
  return 2;
}
async function applyConstraintChaos(database, phase) {
  await database.execute("PRAGMA foreign_keys = ON");
  const validPrefix = `valid-${phase}`;
  const existingValidRows = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_constraints WHERE id LIKE ?",
    `${validPrefix}-%`
  );
  const runSeq = existingValidRows?.count ?? 0;
  const validId = `${validPrefix}-${runSeq}`;
  const uniqValue = `uniq-${phase}-${runSeq}`;
  const before = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_constraints"
  );
  await database.execute(
    `INSERT INTO fuzz_constraints (id, must_not_null, qty, uniq)
		VALUES (?, ?, ?, ?)
		ON CONFLICT(id) DO UPDATE SET
			must_not_null = excluded.must_not_null,
			qty = excluded.qty,
			uniq = excluded.uniq`,
    validId,
    "ok",
    phase,
    uniqValue
  );
  const attempts = [
    {
      name: `not-null-${phase}-${runSeq}`,
      sql: "INSERT INTO fuzz_constraints (id, must_not_null, qty, uniq) VALUES (?, ?, ?, ?)",
      args: [
        `bad-null-${phase}-${runSeq}`,
        null,
        1,
        `bad-null-${phase}-${runSeq}`
      ]
    },
    {
      name: `check-${phase}-${runSeq}`,
      sql: "INSERT INTO fuzz_constraints (id, must_not_null, qty, uniq) VALUES (?, ?, ?, ?)",
      args: [
        `bad-check-${phase}-${runSeq}`,
        "ok",
        -1,
        `bad-check-${phase}-${runSeq}`
      ]
    },
    {
      name: `unique-${phase}-${runSeq}`,
      sql: "INSERT INTO fuzz_constraints (id, must_not_null, qty, uniq) VALUES (?, ?, ?, ?)",
      args: [`bad-unique-${phase}-${runSeq}`, "ok", 1, uniqValue]
    }
  ];
  for (const attempt of attempts) {
    const attemptBefore = await queryOne2(
      database,
      "SELECT COUNT(*) AS count FROM fuzz_constraints"
    );
    let failed = false;
    try {
      await database.execute(attempt.sql, ...attempt.args);
    } catch {
      failed = true;
    }
    const attemptAfter = await queryOne2(
      database,
      "SELECT COUNT(*) AS count FROM fuzz_constraints"
    );
    await database.execute(
      `INSERT INTO fuzz_constraint_attempts (
				name, expected_failed, actually_failed, before_count, after_count
			) VALUES (?, 1, ?, ?, ?)`,
      attempt.name,
      failed ? 1 : 0,
      attemptBefore?.count ?? 0,
      attemptAfter?.count ?? 0
    );
  }
  const after = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_constraints"
  );
  if ((after?.count ?? 0) !== (before?.count ?? 0) + 1) {
    await database.execute(
      `INSERT INTO fuzz_constraint_attempts (
				name, expected_failed, actually_failed, before_count, after_count
			) VALUES (?, 0, 0, ?, ?)`,
      `valid-count-${phase}-${runSeq}`,
      before?.count ?? 0,
      after?.count ?? 0
    );
  }
  const parentId = `fk-parent-${phase}-${runSeq}`;
  const childId = `fk-child-${phase}-${runSeq}`;
  await database.execute(
    "INSERT INTO fuzz_fk_parent (id) VALUES (?) ON CONFLICT(id) DO NOTHING",
    parentId
  );
  await database.execute(
    `INSERT INTO fuzz_fk_child (id, parent_id)
		VALUES (?, ?)
		ON CONFLICT(id) DO UPDATE SET parent_id = excluded.parent_id`,
    childId,
    parentId
  );
  const childBeforeDelete = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_fk_child WHERE parent_id = ?",
    parentId
  );
  await database.execute("DELETE FROM fuzz_fk_parent WHERE id = ?", parentId);
  const childAfterDelete = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_fk_child WHERE parent_id = ?",
    parentId
  );
  await recordProbe(
    database,
    phase,
    "constraints",
    "fk-cascade-delete",
    0,
    childAfterDelete?.count ?? -1,
    (childBeforeDelete?.count ?? 0) !== 1 || (childAfterDelete?.count ?? -1) !== 0
  );
  const fkBefore = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_fk_child"
  );
  let fkFailed = false;
  try {
    await database.execute(
      "INSERT INTO fuzz_fk_child (id, parent_id) VALUES (?, ?)",
      `fk-orphan-${phase}-${runSeq}`,
      `missing-parent-${phase}-${runSeq}`
    );
  } catch {
    fkFailed = true;
  }
  const fkAfter = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_fk_child"
  );
  await recordProbe(
    database,
    phase,
    "constraints",
    "fk-failure-isolation",
    `${fkBefore?.count ?? 0}:failed`,
    `${fkAfter?.count ?? 0}:${fkFailed ? "failed" : "inserted"}`,
    !fkFailed || (fkAfter?.count ?? 0) !== (fkBefore?.count ?? 0)
  );
  return attempts.length + 3;
}
async function applyPragmaProbe(database, phase) {
  let ops = 0;
  for (const [name, setupSql, checkSql, expected] of [
    [
      "journal_mode",
      "PRAGMA journal_mode = DELETE",
      "PRAGMA journal_mode",
      "nonempty"
    ],
    [
      "synchronous",
      "PRAGMA synchronous = NORMAL",
      "PRAGMA synchronous",
      "nonempty"
    ],
    [
      "cache_size",
      "PRAGMA cache_size = -2000",
      "PRAGMA cache_size",
      "-2000"
    ],
    [
      "foreign_keys",
      "PRAGMA foreign_keys = ON",
      "PRAGMA foreign_keys",
      "1"
    ],
    ["auto_vacuum", "PRAGMA auto_vacuum", "PRAGMA auto_vacuum", "nonempty"]
  ]) {
    try {
      await database.execute(setupSql);
      const rows = await database.execute(checkSql);
      const actual = String(firstColumn(rows[0]) ?? "");
      await recordProbe(
        database,
        phase,
        "pragma",
        name,
        expected,
        actual,
        expected === "nonempty" ? actual.length === 0 : actual !== expected
      );
    } catch (err) {
      await recordProbe(
        database,
        phase,
        "pragma",
        name,
        expected,
        err instanceof Error ? err.message : "unknown error",
        true
      );
    }
    ops += 1;
  }
  return ops;
}
async function applySavepointScenario(database, phase) {
  const keepId = `save-keep-${phase}`;
  const rolledBackId = `save-rolled-back-${phase}`;
  await database.execute("BEGIN");
  try {
    await database.execute(
      "INSERT INTO fuzz_savepoints (id, value) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET value = excluded.value",
      keepId,
      phase
    );
    await database.execute("SAVEPOINT sp_rollback_probe");
    await database.execute(
      "INSERT INTO fuzz_savepoints (id, value) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET value = excluded.value",
      rolledBackId,
      999e3 + phase
    );
    await database.execute(
      "UPDATE fuzz_savepoints SET value = value + 1000 WHERE id = ?",
      keepId
    );
    await database.execute("ROLLBACK TO sp_rollback_probe");
    await database.execute("RELEASE sp_rollback_probe");
    await database.execute("COMMIT");
  } catch (err) {
    await database.execute("ROLLBACK").catch(() => void 0);
    throw err;
  }
  await database.execute(
    `INSERT INTO fuzz_savepoint_expectations (id, present, value)
		VALUES (?, 1, ?)
		ON CONFLICT(id) DO UPDATE SET present = 1, value = excluded.value`,
    keepId,
    phase
  );
  await database.execute(
    `INSERT INTO fuzz_savepoint_expectations (id, present, value)
		VALUES (?, 0, 0)
		ON CONFLICT(id) DO UPDATE SET present = 0, value = 0`,
    rolledBackId
  );
  return 5;
}
async function applyIdempotentReplay(database, phase) {
  const targetId = `idem-target-${phase % 3}`;
  await database.execute(
    "INSERT OR IGNORE INTO fuzz_idempotent_targets (id, value) VALUES (?, 0)",
    targetId
  );
  for (let i = 0; i < 8; i += 1) {
    const opId = `idem-${phase}-${i}`;
    const amount = phase + i + 1;
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await transaction(database, async () => {
        const existing = await queryOne2(
          database,
          "SELECT op_id FROM fuzz_idempotent_ops WHERE op_id = ?",
          opId
        );
        if (!existing) {
          await database.execute(
            "INSERT INTO fuzz_idempotent_ops (op_id, target_id, amount) VALUES (?, ?, ?)",
            opId,
            targetId,
            amount
          );
          await database.execute(
            "UPDATE fuzz_idempotent_targets SET value = value + ? WHERE id = ?",
            amount,
            targetId
          );
        }
      });
    }
  }
  return 24;
}
async function ensureRelationalSeed(database) {
  await transaction(database, async () => {
    for (let i = 0; i < 8; i += 1) {
      await database.execute(
        "INSERT OR IGNORE INTO fuzz_rel_users (id, name) VALUES (?, ?)",
        `user-${i}`,
        `User ${i}`
      );
    }
    for (let i = 0; i < 12; i += 1) {
      const productId = `product-${i}`;
      const initialQty = 1e4;
      await database.execute(
        "INSERT OR IGNORE INTO fuzz_rel_products (id, price) VALUES (?, ?)",
        productId,
        (i + 1) * 7
      );
      await database.execute(
        `INSERT OR IGNORE INTO fuzz_inventory (
					product_id, initial_qty, sold_qty, stock_qty
				) VALUES (?, ?, 0, ?)`,
        productId,
        initialQty,
        initialQty
      );
    }
  });
}
async function applyRelationalOrder(database, opts) {
  await ensureRelationalSeed(database);
  const orderPrefix = `order-${opts.phase}-${opts.localIndex}`;
  const existingOrders = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_orders WHERE id LIKE ?",
    `${orderPrefix}-%`
  );
  const orderId = `${orderPrefix}-${existingOrders?.count ?? 0}`;
  const userId = `user-${intBetween(opts.rng, 0, 7)}`;
  const itemCount = intBetween(opts.rng, 1, 4);
  let total = 0;
  await transaction(database, async () => {
    await database.execute(
      "INSERT INTO fuzz_orders (id, user_id, total, status) VALUES (?, ?, 0, 'open')",
      orderId,
      userId
    );
    for (let i = 0; i < itemCount; i += 1) {
      const productId = `product-${intBetween(opts.rng, 0, 11)}`;
      const product = await queryOne2(
        database,
        "SELECT price FROM fuzz_rel_products WHERE id = ?",
        productId
      );
      const quantity = intBetween(opts.rng, 1, 5);
      const price = product?.price ?? 0;
      total += price * quantity;
      await database.execute(
        `INSERT INTO fuzz_order_items (
					order_id, product_id, quantity, price
				) VALUES (?, ?, ?, ?)`,
        orderId,
        productId,
        quantity,
        price
      );
      await database.execute(
        `UPDATE fuzz_inventory
				SET sold_qty = sold_qty + ?, stock_qty = stock_qty - ?
				WHERE product_id = ?`,
        quantity,
        quantity,
        productId
      );
    }
    await database.execute(
      "UPDATE fuzz_orders SET total = ?, status = 'paid' WHERE id = ?",
      total,
      orderId
    );
    await database.execute(
      "INSERT INTO fuzz_payments (order_id, amount, status) VALUES (?, ?, 'captured')",
      orderId,
      total
    );
  });
  return itemCount + 4;
}
async function applyRollbackProbe(database, phase, rowCount = 20) {
  const before = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_items WHERE item_key LIKE ?",
    `rollback-${phase}-%`
  );
  await database.execute("BEGIN");
  try {
    for (let i = 0; i < rowCount; i += 1) {
      await database.execute(
        `INSERT INTO fuzz_items (
					item_key, value, version, update_count, payload, payload_checksum,
					payload_bytes, updated_at
				) VALUES (?, ?, 1, 1, ?, ?, ?, ?)`,
        `rollback-${phase}-${i}`,
        "should-not-survive",
        "rollback-payload",
        checksum("rollback-payload"),
        "rollback-payload".length,
        Date.now()
      );
    }
    throw new Error("intentional rollback probe");
  } catch {
    await database.execute("ROLLBACK");
  }
  const after = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_items WHERE item_key LIKE ?",
    `rollback-${phase}-%`
  );
  await database.execute(
    `INSERT INTO fuzz_constraint_attempts (
			name, expected_failed, actually_failed, before_count, after_count
		) VALUES (?, 1, 1, ?, ?)`,
    `rollback-probe-${phase}`,
    before?.count ?? 0,
    after?.count ?? 0
  );
  return rowCount;
}
async function applyNastyScript(database, opts) {
  const growKey = `nasty-grow-${opts.phase}`;
  let ops = 0;
  const growMax = Math.min(opts.maxPayloadBytes, 131072);
  const growSizes = PAGE_BOUNDARY_SIZES.filter((size) => size <= growMax);
  if (!growSizes.includes(1)) growSizes.unshift(1);
  if (!growSizes.includes(growMax)) growSizes.push(growMax);
  for (const size of growSizes) {
    await applyItemOperation(database, {
      seed: opts.seed,
      phase: opts.phase,
      localIndex: 5e4 + size,
      kind: "upsert",
      itemKey: growKey,
      payloadBytes: Math.min(size, opts.maxPayloadBytes)
    });
    ops += 1;
  }
  const hotUpdates = opts.intense ? 1e4 : 250;
  await applyHotUpdates(database, {
    seed: opts.seed,
    phase: opts.phase,
    localIndex: 6e4,
    itemKey: `nasty-hot-${opts.phase}`,
    updates: hotUpdates,
    payloadBytes: Math.min(1024, opts.maxPayloadBytes)
  });
  ops += hotUpdates;
  if (opts.intense) {
    await database.execute(
      "CREATE INDEX IF NOT EXISTS idx_nasty_heavy_write ON fuzz_items(value, version)"
    );
    for (let i = 0; i < 1e4; i += 1) {
      await applyItemOperation(database, {
        seed: opts.seed,
        phase: opts.phase,
        localIndex: 12e4 + i,
        kind: "upsert",
        itemKey: `nasty-bulk-${opts.phase}-${i}`,
        payloadBytes: Math.min(256, opts.maxPayloadBytes)
      });
      ops += 1;
    }
    for (let i = 0; i < 1e4; i += 2) {
      await applyItemOperation(database, {
        seed: opts.seed,
        phase: opts.phase,
        localIndex: 14e4 + i,
        kind: "delete",
        itemKey: `nasty-bulk-${opts.phase}-${i}`,
        payloadBytes: 1
      });
      ops += 1;
    }
    await database.execute("DROP INDEX IF EXISTS idx_nasty_heavy_write");
  }
  const rollbackRows = opts.intense ? 1e3 : 20;
  await applyRollbackProbe(database, opts.phase, rollbackRows);
  ops += rollbackRows;
  return ops;
}
async function applyDeterministicNastyScript(database, opts) {
  let ops = 0;
  const growId = `nasty-script-grow-${opts.phase}`;
  const maxGrowBytes = Math.min(opts.maxPayloadBytes, 131072);
  let finalGrowPayload = "";
  await transaction(database, async () => {
    for (let i = 0; i < 256; i += 1) {
      const size = Math.max(
        1,
        Math.floor(1 + (maxGrowBytes - 1) * i / 255)
      );
      const payload2 = payloadFor(
        opts.seed,
        opts.phase,
        16e4 + i,
        size
      );
      finalGrowPayload = payload2;
      await database.execute(
        `INSERT INTO fuzz_edge_payloads (
					id, kind, payload, payload_checksum, payload_bytes, updated_at
				) VALUES (?, 'nasty-grow', ?, ?, ?, ?)
				ON CONFLICT(id) DO UPDATE SET
					payload = excluded.payload,
					payload_checksum = excluded.payload_checksum,
					payload_bytes = excluded.payload_bytes,
					updated_at = excluded.updated_at`,
        growId,
        payload2,
        checksum(payload2),
        payload2.length,
        Date.now()
      );
      ops += 1;
    }
    await database.execute(
      `INSERT INTO fuzz_edge_expectations (
				id, present, payload_checksum, payload_bytes
			) VALUES (?, 1, ?, ?)
			ON CONFLICT(id) DO UPDATE SET
				present = 1,
				payload_checksum = excluded.payload_checksum,
				payload_bytes = excluded.payload_bytes`,
      growId,
      checksum(finalGrowPayload),
      finalGrowPayload.length
    );
  });
  const counterId = `nasty-counter-${opts.phase}`;
  await database.execute(
    "INSERT INTO fuzz_nasty_counter (id, value) VALUES (?, 0) ON CONFLICT(id) DO UPDATE SET value = 0",
    counterId
  );
  await transaction(database, async () => {
    for (let i = 0; i < 1e4; i += 1) {
      await database.execute(
        "UPDATE fuzz_nasty_counter SET value = value + 1 WHERE id = ?",
        counterId
      );
      ops += 1;
    }
  });
  const counter2 = await queryOne2(
    database,
    "SELECT value FROM fuzz_nasty_counter WHERE id = ?",
    counterId
  );
  await recordProbe(
    database,
    opts.phase,
    "nasty-script",
    "same-row-10k-updates",
    1e4,
    counter2?.value ?? -1,
    (counter2?.value ?? -1) !== 1e4
  );
  const groupId = `nasty-bulk-${opts.phase}`;
  await database.execute(
    "CREATE INDEX IF NOT EXISTS idx_fuzz_nasty_rows_group_n ON fuzz_nasty_rows(group_id, n)"
  );
  await transaction(database, async () => {
    for (let i = 0; i < 1e4; i += 1) {
      await database.execute(
        `INSERT INTO fuzz_nasty_rows (group_id, n, payload)
				VALUES (?, ?, ?)
				ON CONFLICT(group_id, n) DO UPDATE SET payload = excluded.payload`,
        groupId,
        i,
        payloadFor(opts.seed, opts.phase, 17e4 + i, 64)
      );
      ops += 1;
    }
    await database.execute(
      "DELETE FROM fuzz_nasty_rows WHERE group_id = ? AND n % 2 = 0",
      groupId
    );
    ops += 1;
  });
  await database.execute("DROP INDEX IF EXISTS idx_fuzz_nasty_rows_group_n");
  const remaining = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_nasty_rows WHERE group_id = ?",
    groupId
  );
  await recordProbe(
    database,
    opts.phase,
    "nasty-script",
    "insert-10k-delete-every-other",
    5e3,
    remaining?.count ?? -1,
    (remaining?.count ?? -1) !== 5e3
  );
  const indexLeft = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM sqlite_master WHERE type = 'index' AND name = 'idx_fuzz_nasty_rows_group_n'"
  );
  await recordProbe(
    database,
    opts.phase,
    "nasty-script",
    "create-drop-index-around-heavy-writes",
    0,
    indexLeft?.count ?? -1,
    (indexLeft?.count ?? -1) !== 0
  );
  const rollbackGroupId = `nasty-rollback-${opts.phase}`;
  const beforeRollback = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_nasty_rows WHERE group_id = ?",
    rollbackGroupId
  );
  await database.execute("BEGIN");
  try {
    for (let i = 0; i < 1e3; i += 1) {
      await database.execute(
        "INSERT INTO fuzz_nasty_rows (group_id, n, payload) VALUES (?, ?, ?)",
        rollbackGroupId,
        i,
        "rollback"
      );
      ops += 1;
    }
    await database.execute("ROLLBACK");
  } catch (err) {
    await database.execute("ROLLBACK").catch(() => void 0);
    throw err;
  }
  const afterRollback = await queryOne2(
    database,
    "SELECT COUNT(*) AS count FROM fuzz_nasty_rows WHERE group_id = ?",
    rollbackGroupId
  );
  await recordProbe(
    database,
    opts.phase,
    "nasty-script",
    "rollback-1k-inserts",
    beforeRollback?.count ?? 0,
    afterRollback?.count ?? -1,
    (afterRollback?.count ?? -1) !== (beforeRollback?.count ?? 0)
  );
  return ops;
}
function shouldRunDeepScenario(mode2, scenario) {
  return mode2 === scenario || mode2 === "kitchen-sink" || mode2 === "nasty";
}
async function applyDeepScenarios(database, opts) {
  const runScenario = async (name, fn) => {
    try {
      return await fn();
    } catch (error) {
      const detail = error instanceof Error ? error.message : typeof error === "string" ? error : String(error);
      throw new Error(
        `deep scenario ${name} failed in mode ${opts.mode} during phase ${opts.phase}: ${detail}`,
        { cause: error }
      );
    }
  };
  if (shouldRunDeepScenario(opts.mode, "edge") || opts.mode === "payloads") {
    opts.ops.edgePayload = (opts.ops.edgePayload ?? 0) + await runScenario(
      "edge",
      () => applyEdgePayloads(database, opts)
    );
  }
  if (opts.mode === "actual-nul") {
    opts.ops.actualNul = (opts.ops.actualNul ?? 0) + await runScenario(
      "actual-nul",
      () => applyActualNulPayload(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "fragmentation")) {
    opts.ops.fragmentation = (opts.ops.fragmentation ?? 0) + await runScenario(
      "fragmentation",
      () => applyFragmentationChurn(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "schema")) {
    opts.ops.schema = (opts.ops.schema ?? 0) + await runScenario(
      "schema",
      () => applySchemaChurn(database, opts.phase)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "index")) {
    opts.ops.index = (opts.ops.index ?? 0) + await runScenario("index", () => applyIndexProbe(database, opts));
  }
  if (shouldRunDeepScenario(opts.mode, "constraints")) {
    opts.ops.constraints = (opts.ops.constraints ?? 0) + await runScenario(
      "constraints",
      () => applyConstraintChaos(database, opts.phase)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "savepoints")) {
    opts.ops.savepoints = (opts.ops.savepoints ?? 0) + await runScenario(
      "savepoints",
      () => applySavepointScenario(database, opts.phase)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "pragma")) {
    opts.ops.pragma = (opts.ops.pragma ?? 0) + await runScenario(
      "pragma",
      () => applyPragmaProbe(database, opts.phase)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "prepared")) {
    opts.ops.prepared = (opts.ops.prepared ?? 0) + await runScenario(
      "prepared",
      () => applyPreparedChurn(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "growth")) {
    opts.ops.growth = (opts.ops.growth ?? 0) + await runScenario(
      "growth",
      () => applyGrowthProbe(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "readwrite")) {
    opts.ops.readwrite = (opts.ops.readwrite ?? 0) + await runScenario(
      "readwrite",
      () => applyReadWriteProbe(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "truncate")) {
    opts.ops.truncate = (opts.ops.truncate ?? 0) + await runScenario(
      "truncate",
      () => applyTruncateRecreateProbe(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "boundary-keys")) {
    opts.ops.boundaryKeys = (opts.ops.boundaryKeys ?? 0) + await runScenario(
      "boundary-keys",
      () => applyBoundaryKeys(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "relational")) {
    const orders = Math.max(1, Math.floor(opts.iterations / 20));
    for (let i = 0; i < orders; i += 1) {
      opts.ops.relational = (opts.ops.relational ?? 0) + await runScenario(
        "relational",
        () => applyRelationalOrder(database, {
          phase: opts.phase,
          localIndex: i,
          rng: opts.rng
        })
      );
    }
  }
  if (opts.mode === "kitchen-sink" || opts.mode === "nasty") {
    opts.ops.idempotent = (opts.ops.idempotent ?? 0) + await runScenario(
      "idempotent",
      () => applyIdempotentReplay(database, opts.phase)
    );
    opts.ops.nasty = (opts.ops.nasty ?? 0) + await runScenario(
      "nasty",
      () => applyNastyScript(database, {
        ...opts,
        intense: opts.mode === "nasty"
      })
    );
  }
  if (opts.mode === "nasty-script") {
    opts.ops.nasty = (opts.ops.nasty ?? 0) + await runScenario(
      "nasty-script",
      () => applyDeterministicNastyScript(database, opts)
    );
  }
  if (shouldRunDeepScenario(opts.mode, "shadow")) {
    opts.ops.shadow = (opts.ops.shadow ?? 0) + await runScenario(
      "shadow",
      () => updateShadowChecksums(database, opts.phase)
    );
  }
}
function chooseKind(mode2, rng) {
  const roll = rng();
  if (mode2 === "transactions") {
    if (roll < 0.55) return "transfer";
    if (roll < 0.75) return "upsert";
    if (roll < 0.9) return "update";
    return "delete";
  }
  if (mode2 === "hot") {
    if (roll < 0.6) return "hot";
    if (roll < 0.75) return "upsert";
    if (roll < 0.9) return "update";
    return "delete";
  }
  if (mode2 === "payloads") {
    if (roll < 0.4) return "upsert";
    if (roll < 0.7) return "insert";
    if (roll < 0.9) return "update";
    return "delete";
  }
  if (roll < 0.2) return "insert";
  if (roll < 0.45) return "update";
  if (roll < 0.65) return "delete";
  if (roll < 0.85) return "upsert";
  if (roll < 0.95) return "hot";
  return "transfer";
}
async function validate(database) {
  const integrity = await queryOne2(
    database,
    "PRAGMA integrity_check"
  );
  const quick = await queryOne2(
    database,
    "PRAGMA quick_check"
  );
  const totals = await queryOne2(
    database,
    `WITH latest AS (
			SELECT e.*
			FROM fuzz_item_events e
			JOIN (
				SELECT item_key, MAX(seq) AS seq
				FROM fuzz_item_events
				GROUP BY item_key
			) m ON m.item_key = e.item_key AND m.seq = e.seq
		)
		SELECT
			(SELECT COUNT(*) FROM fuzz_item_events) AS total_events,
			(SELECT COUNT(*) FROM fuzz_items) AS active_rows,
			(SELECT COUNT(*) FROM latest WHERE present = 1) AS expected_rows,
			(SELECT COALESCE(SUM(version), 0) FROM fuzz_items) AS actual_version_sum,
			(SELECT COALESCE(SUM(version), 0) FROM latest WHERE present = 1) AS expected_version_sum,
			(SELECT COALESCE(SUM(payload_checksum), 0) FROM fuzz_items) AS actual_payload_checksum_sum,
			(SELECT COALESCE(SUM(payload_checksum), 0) FROM latest WHERE present = 1) AS expected_payload_checksum_sum`
  );
  const mismatches = await queryOne2(
    database,
    `WITH latest AS (
			SELECT e.*
			FROM fuzz_item_events e
			JOIN (
				SELECT item_key, MAX(seq) AS seq
				FROM fuzz_item_events
				GROUP BY item_key
			) m ON m.item_key = e.item_key AND m.seq = e.seq
		)
		SELECT
			(
				SELECT COUNT(*)
				FROM latest l
				LEFT JOIN fuzz_items i ON i.item_key = l.item_key
				WHERE l.present = 1 AND i.item_key IS NULL
			) AS missing_rows,
			(
				SELECT COUNT(*)
				FROM fuzz_items i
				LEFT JOIN latest l ON l.item_key = i.item_key
				WHERE l.item_key IS NULL OR l.present = 0
			) AS extra_rows,
			(
				SELECT COUNT(*)
				FROM latest l
				JOIN fuzz_items i ON i.item_key = l.item_key
				WHERE l.present = 1
					AND (
						i.value != l.value OR
						i.version != l.version OR
						i.update_count != l.update_count OR
						i.payload_checksum != l.payload_checksum OR
						i.payload_bytes != l.payload_bytes
					)
			) AS mismatched_rows,
			(
				SELECT COUNT(*)
				FROM (
					SELECT item_key
					FROM fuzz_items
					GROUP BY item_key
					HAVING COUNT(*) > 1
				)
			) AS duplicate_keys`
  );
  const accounts = await queryOne2(
    database,
    `SELECT
			COUNT(*) AS account_count,
			COALESCE(SUM(balance), 0) AS account_balance_sum,
			(
				SELECT COUNT(*)
				FROM fuzz_transfer_events
				WHERE balance_sum_before != ? OR balance_sum_after != ?
			) AS account_balance_mismatch
		FROM fuzz_accounts`,
    ACCOUNT_COUNT * ACCOUNT_INITIAL_BALANCE,
    ACCOUNT_COUNT * ACCOUNT_INITIAL_BALANCE
  );
  const edge = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_edge_payloads) AS edge_rows,
			(SELECT COUNT(*) FROM fuzz_edge_expectations WHERE present = 1) AS edge_expected_rows,
			(
				SELECT COUNT(*)
				FROM fuzz_edge_expectations e
				LEFT JOIN fuzz_edge_payloads p ON p.id = e.id
				WHERE
					(e.present = 1 AND p.id IS NULL) OR
					(e.present = 0 AND p.id IS NOT NULL) OR
					(e.present = 1 AND (
						p.payload_checksum != e.payload_checksum OR
						p.payload_bytes != e.payload_bytes
					))
			) AS edge_mismatches`
  );
  const indexProbe = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_indexed) AS index_rows,
			(
				SELECT COUNT(*)
				FROM (
					SELECT id FROM fuzz_indexed
					WHERE tenant = 'tenant-1' AND bucket BETWEEN 2 AND 8 AND score BETWEEN -250 AND 250
					EXCEPT
					SELECT id FROM fuzz_indexed NOT INDEXED
					WHERE tenant = 'tenant-1' AND bucket BETWEEN 2 AND 8 AND score BETWEEN -250 AND 250
				)
			) + (
				SELECT COUNT(*)
				FROM (
					SELECT id FROM fuzz_indexed NOT INDEXED
					WHERE tenant = 'tenant-1' AND bucket BETWEEN 2 AND 8 AND score BETWEEN -250 AND 250
					EXCEPT
					SELECT id FROM fuzz_indexed
					WHERE tenant = 'tenant-1' AND bucket BETWEEN 2 AND 8 AND score BETWEEN -250 AND 250
				)
			) AS index_mismatches`
  );
  const relational = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_orders) AS relational_orders,
			(
				SELECT COUNT(*)
				FROM fuzz_orders o
				LEFT JOIN (
					SELECT order_id, COALESCE(SUM(quantity * price), 0) AS item_total
					FROM fuzz_order_items
					GROUP BY order_id
				) i ON i.order_id = o.id
				WHERE o.total != COALESCE(i.item_total, 0)
			) + (
				SELECT COUNT(*)
				FROM fuzz_orders o
				LEFT JOIN (
					SELECT order_id, COALESCE(SUM(amount), 0) AS payment_total
					FROM fuzz_payments
					WHERE status = 'captured'
					GROUP BY order_id
				) p ON p.order_id = o.id
				WHERE o.status = 'paid' AND o.total != COALESCE(p.payment_total, 0)
			) + (
				SELECT COUNT(*)
				FROM fuzz_inventory
				WHERE initial_qty != sold_qty + stock_qty OR stock_qty < 0
			) AS relational_mismatches`
  );
  const constraints = await queryOne2(
    database,
    `SELECT
			COUNT(*) AS constraint_attempts,
			COALESCE(SUM(
				CASE
					WHEN expected_failed = 1 AND (actually_failed != 1 OR before_count != after_count) THEN 1
					WHEN expected_failed = 0 AND after_count != before_count + 1 THEN 1
					ELSE 0
				END
			), 0) AS constraint_leaks
		FROM fuzz_constraint_attempts`
  );
  const savepoints = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_savepoints) AS savepoint_rows,
			(
				SELECT COUNT(*)
				FROM fuzz_savepoint_expectations e
				LEFT JOIN fuzz_savepoints s ON s.id = e.id
				WHERE
					(e.present = 1 AND s.id IS NULL) OR
					(e.present = 0 AND s.id IS NOT NULL) OR
					(e.present = 1 AND s.value != e.value)
			) AS savepoint_mismatches`
  );
  const idempotency = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_idempotent_ops) AS idempotent_ops,
			(
				SELECT COUNT(*)
				FROM fuzz_idempotent_targets t
				LEFT JOIN (
					SELECT target_id, COALESCE(SUM(amount), 0) AS expected
					FROM fuzz_idempotent_ops
					GROUP BY target_id
				) o ON o.target_id = t.id
				WHERE t.value != COALESCE(o.expected, 0)
			) AS idempotent_mismatches`
  );
  const schema = await queryOne2(
    database,
    `SELECT
			COUNT(*) AS schema_objects,
			COALESCE(SUM(CASE WHEN m.name IS NULL THEN 1 ELSE 0 END), 0) AS schema_missing_objects
		FROM fuzz_schema_registry r
		LEFT JOIN sqlite_master m ON m.name = r.name AND m.type = r.type`
  );
  const probes = await queryOne2(
    database,
    `SELECT
			COUNT(*) AS probe_rows,
			COALESCE(SUM(mismatch), 0) AS probe_mismatches
		FROM fuzz_probe_results`
  );
  const prepared = await queryOne2(
    database,
    `SELECT
			(SELECT COUNT(*) FROM fuzz_prepared_churn) AS prepared_rows,
			(
				SELECT COUNT(*)
				FROM fuzz_prepared_expectations e
				LEFT JOIN fuzz_prepared_churn p ON p.id = e.id
				WHERE
					p.id IS NULL OR
					p.value != e.value OR
					p.payload_checksum != e.payload_checksum
			) AS prepared_mismatches`
  );
  const shadow = await queryOne2(
    database,
    `WITH recomputed AS (
			SELECT 'items' AS name,
				COUNT(*) AS row_count,
				COALESCE(SUM(payload_checksum + version + update_count), 0) AS value
			FROM fuzz_items
			UNION ALL
			SELECT 'edge' AS name,
				COUNT(*) AS row_count,
				COALESCE(SUM(payload_checksum + payload_bytes), 0) AS value
			FROM fuzz_edge_payloads
		)
		SELECT
			(SELECT COUNT(*) FROM fuzz_shadow_checksums) AS shadow_rows,
			(
				SELECT COUNT(*)
				FROM fuzz_shadow_checksums s
				JOIN recomputed r ON r.name = s.name
				WHERE s.value != r.value OR s.row_count != r.row_count
			) AS shadow_mismatches`
  );
  const summary = {
    totalEvents: totals?.total_events ?? 0,
    activeRows: totals?.active_rows ?? 0,
    expectedRows: totals?.expected_rows ?? 0,
    missingRows: mismatches?.missing_rows ?? 0,
    extraRows: mismatches?.extra_rows ?? 0,
    mismatchedRows: mismatches?.mismatched_rows ?? 0,
    duplicateKeys: mismatches?.duplicate_keys ?? 0,
    actualVersionSum: totals?.actual_version_sum ?? 0,
    expectedVersionSum: totals?.expected_version_sum ?? 0,
    actualPayloadChecksumSum: totals?.actual_payload_checksum_sum ?? 0,
    expectedPayloadChecksumSum: totals?.expected_payload_checksum_sum ?? 0,
    accountCount: accounts?.account_count ?? 0,
    accountBalanceSum: accounts?.account_balance_sum ?? 0,
    expectedAccountBalanceSum: ACCOUNT_COUNT * ACCOUNT_INITIAL_BALANCE,
    accountBalanceMismatch: accounts?.account_balance_mismatch ?? 0,
    integrityCheck: integrity?.integrity_check ?? "missing",
    quickCheck: quick?.quick_check ?? "missing",
    edgeRows: edge?.edge_rows ?? 0,
    edgeExpectedRows: edge?.edge_expected_rows ?? 0,
    edgeMismatches: edge?.edge_mismatches ?? 0,
    indexRows: indexProbe?.index_rows ?? 0,
    indexMismatches: indexProbe?.index_mismatches ?? 0,
    relationalOrders: relational?.relational_orders ?? 0,
    relationalMismatches: relational?.relational_mismatches ?? 0,
    constraintAttempts: constraints?.constraint_attempts ?? 0,
    constraintLeaks: constraints?.constraint_leaks ?? 0,
    savepointRows: savepoints?.savepoint_rows ?? 0,
    savepointMismatches: savepoints?.savepoint_mismatches ?? 0,
    idempotentOps: idempotency?.idempotent_ops ?? 0,
    idempotentMismatches: idempotency?.idempotent_mismatches ?? 0,
    schemaObjects: schema?.schema_objects ?? 0,
    schemaMissingObjects: schema?.schema_missing_objects ?? 0,
    probeRows: probes?.probe_rows ?? 0,
    probeMismatches: probes?.probe_mismatches ?? 0,
    preparedRows: prepared?.prepared_rows ?? 0,
    preparedMismatches: prepared?.prepared_mismatches ?? 0,
    shadowRows: shadow?.shadow_rows ?? 0,
    shadowMismatches: shadow?.shadow_mismatches ?? 0
  };
  return summary;
}
async function debugItemMismatches(database, limit = 5) {
  const rows = await database.execute(
    `WITH latest AS (
			SELECT e.*
			FROM fuzz_item_events e
			JOIN (
				SELECT item_key, MAX(seq) AS seq
				FROM fuzz_item_events
				GROUP BY item_key
			) m ON m.item_key = e.item_key AND m.seq = e.seq
		)
		SELECT
			COALESCE(l.item_key, i.item_key) AS item_key,
			i.value AS actual_value,
			l.value AS expected_value,
			i.version AS actual_version,
			l.version AS expected_version,
			i.update_count AS actual_update_count,
			l.update_count AS expected_update_count,
			i.payload_checksum AS actual_payload_checksum,
			l.payload_checksum AS expected_payload_checksum,
			i.payload_bytes AS actual_payload_bytes,
			l.payload_bytes AS expected_payload_bytes
		FROM latest l
		FULL OUTER JOIN fuzz_items i ON i.item_key = l.item_key
		WHERE
			(l.present = 1 AND i.item_key IS NULL) OR
			((l.item_key IS NULL OR l.present = 0) AND i.item_key IS NOT NULL) OR
			(
				l.present = 1 AND i.item_key IS NOT NULL AND (
					i.value != l.value OR
					i.version != l.version OR
					i.update_count != l.update_count OR
					i.payload_checksum != l.payload_checksum OR
					i.payload_bytes != l.payload_bytes
				)
			)
		ORDER BY COALESCE(l.item_key, i.item_key)
		LIMIT ?`,
    limit
  );
  const itemMismatches = rows.map((row) => ({
    itemKey: row.item_key,
    actualValue: row.actual_value,
    expectedValue: row.expected_value,
    actualVersion: row.actual_version,
    expectedVersion: row.expected_version,
    actualUpdateCount: row.actual_update_count,
    expectedUpdateCount: row.expected_update_count,
    actualPayloadChecksum: row.actual_payload_checksum,
    expectedPayloadChecksum: row.expected_payload_checksum,
    actualPayloadBytes: row.actual_payload_bytes,
    expectedPayloadBytes: row.expected_payload_bytes
  }));
  const recentEventsByKey = {};
  for (const row of itemMismatches) {
    const events = await database.execute(
      `SELECT
				seq,
				phase,
				local_index,
				kind,
				present,
				value,
				version,
				update_count,
				payload_checksum,
				payload_bytes,
				applied
			FROM fuzz_item_events
			WHERE item_key = ?
			ORDER BY seq DESC
			LIMIT 10`,
      row.itemKey
    );
    recentEventsByKey[row.itemKey] = events.map((event19) => ({
      seq: event19.seq,
      phase: event19.phase,
      localIndex: event19.local_index,
      kind: event19.kind,
      present: event19.present,
      value: event19.value,
      version: event19.version,
      updateCount: event19.update_count,
      payloadChecksum: event19.payload_checksum,
      payloadBytes: event19.payload_bytes,
      applied: event19.applied
    }));
  }
  return { itemMismatches, recentEventsByKey };
}
var rawSqliteFuzzer = actor42({
  options: {
    actionTimeout: 3e5
  },
  db: db8({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_items (
					item_key TEXT PRIMARY KEY,
					value TEXT NOT NULL,
					version INTEGER NOT NULL,
					update_count INTEGER NOT NULL,
					payload TEXT NOT NULL,
					payload_checksum INTEGER NOT NULL,
					payload_bytes INTEGER NOT NULL,
					updated_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_item_events (
					seq INTEGER PRIMARY KEY AUTOINCREMENT,
					phase INTEGER NOT NULL,
					local_index INTEGER NOT NULL,
					kind TEXT NOT NULL,
					item_key TEXT NOT NULL,
					present INTEGER NOT NULL,
					value TEXT,
					version INTEGER NOT NULL,
					update_count INTEGER NOT NULL,
					payload_checksum INTEGER NOT NULL,
					payload_bytes INTEGER NOT NULL,
					applied INTEGER NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_fuzz_item_events_key_seq ON fuzz_item_events(item_key, seq)"
      );
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_accounts (
					id TEXT PRIMARY KEY,
					balance INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_transfer_events (
					seq INTEGER PRIMARY KEY AUTOINCREMENT,
					phase INTEGER NOT NULL,
					local_index INTEGER NOT NULL,
					from_account TEXT NOT NULL,
					to_account TEXT NOT NULL,
					amount INTEGER NOT NULL,
					balance_sum_before INTEGER NOT NULL,
					balance_sum_after INTEGER NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_edge_payloads (
					id TEXT PRIMARY KEY,
					kind TEXT NOT NULL,
					payload TEXT NOT NULL,
					payload_checksum INTEGER NOT NULL,
					payload_bytes INTEGER NOT NULL,
					updated_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_edge_expectations (
					id TEXT PRIMARY KEY,
					present INTEGER NOT NULL,
					payload_checksum INTEGER NOT NULL,
					payload_bytes INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_trigger_audit (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					payload_id TEXT NOT NULL,
					old_checksum INTEGER NOT NULL,
					new_checksum INTEGER NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_fuzz_edge_kind_size ON fuzz_edge_payloads(kind, payload_bytes)"
      );
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_indexed (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					tenant TEXT NOT NULL,
					bucket INTEGER NOT NULL,
					score INTEGER NOT NULL,
					label TEXT NOT NULL,
					payload TEXT NOT NULL
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_fuzz_indexed_tenant_bucket_score ON fuzz_indexed(tenant, bucket, score)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_fuzz_indexed_score_label ON fuzz_indexed(score, label)"
      );
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_rel_users (
					id TEXT PRIMARY KEY,
					name TEXT NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_rel_products (
					id TEXT PRIMARY KEY,
					price INTEGER NOT NULL CHECK (price >= 0)
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_orders (
					id TEXT PRIMARY KEY,
					user_id TEXT NOT NULL,
					total INTEGER NOT NULL,
					status TEXT NOT NULL,
					FOREIGN KEY (user_id) REFERENCES fuzz_rel_users(id)
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_order_items (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id TEXT NOT NULL,
					product_id TEXT NOT NULL,
					quantity INTEGER NOT NULL CHECK (quantity > 0),
					price INTEGER NOT NULL CHECK (price >= 0),
					FOREIGN KEY (order_id) REFERENCES fuzz_orders(id),
					FOREIGN KEY (product_id) REFERENCES fuzz_rel_products(id)
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_payments (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id TEXT NOT NULL,
					amount INTEGER NOT NULL,
					status TEXT NOT NULL,
					FOREIGN KEY (order_id) REFERENCES fuzz_orders(id)
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_inventory (
					product_id TEXT PRIMARY KEY,
					initial_qty INTEGER NOT NULL,
					sold_qty INTEGER NOT NULL,
					stock_qty INTEGER NOT NULL,
					FOREIGN KEY (product_id) REFERENCES fuzz_rel_products(id)
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_constraints (
					id TEXT PRIMARY KEY,
					must_not_null TEXT NOT NULL,
					qty INTEGER NOT NULL CHECK (qty >= 0),
					uniq TEXT NOT NULL UNIQUE
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_constraint_attempts (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL,
					expected_failed INTEGER NOT NULL,
					actually_failed INTEGER NOT NULL,
					before_count INTEGER NOT NULL,
					after_count INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_fk_parent (
					id TEXT PRIMARY KEY
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_fk_child (
					id TEXT PRIMARY KEY,
					parent_id TEXT NOT NULL,
					FOREIGN KEY (parent_id) REFERENCES fuzz_fk_parent(id) ON DELETE CASCADE
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_savepoints (
					id TEXT PRIMARY KEY,
					value INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_savepoint_expectations (
					id TEXT PRIMARY KEY,
					present INTEGER NOT NULL,
					value INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_idempotent_ops (
					op_id TEXT PRIMARY KEY,
					target_id TEXT NOT NULL,
					amount INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_idempotent_targets (
					id TEXT PRIMARY KEY,
					value INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_schema_registry (
					name TEXT PRIMARY KEY,
					type TEXT NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_probe_results (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					phase INTEGER NOT NULL,
					scenario TEXT NOT NULL,
					name TEXT NOT NULL,
					expected TEXT NOT NULL,
					actual TEXT NOT NULL,
					mismatch INTEGER NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_prepared_churn (
					id TEXT PRIMARY KEY,
					value INTEGER NOT NULL,
					payload TEXT NOT NULL,
					payload_checksum INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_prepared_expectations (
					id TEXT PRIMARY KEY,
					value INTEGER NOT NULL,
					payload_checksum INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_shadow_checksums (
					name TEXT PRIMARY KEY,
					value INTEGER NOT NULL,
					row_count INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_nasty_counter (
					id TEXT PRIMARY KEY,
					value INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS fuzz_nasty_rows (
					group_id TEXT NOT NULL,
					n INTEGER NOT NULL,
					payload TEXT NOT NULL,
					PRIMARY KEY (group_id, n)
				)
			`);
    }
  }),
  actions: {
    reset: async (c) => {
      await c.db.execute("DELETE FROM fuzz_nasty_rows");
      await c.db.execute("DELETE FROM fuzz_nasty_counter");
      await c.db.execute("DELETE FROM fuzz_shadow_checksums");
      await c.db.execute("DELETE FROM fuzz_prepared_expectations");
      await c.db.execute("DELETE FROM fuzz_prepared_churn");
      await c.db.execute("DELETE FROM fuzz_probe_results");
      await c.db.execute("DELETE FROM fuzz_schema_registry");
      await c.db.execute("DELETE FROM fuzz_idempotent_targets");
      await c.db.execute("DELETE FROM fuzz_idempotent_ops");
      await c.db.execute("DELETE FROM fuzz_savepoint_expectations");
      await c.db.execute("DELETE FROM fuzz_savepoints");
      await c.db.execute("DELETE FROM fuzz_fk_child");
      await c.db.execute("DELETE FROM fuzz_fk_parent");
      await c.db.execute("DELETE FROM fuzz_constraint_attempts");
      await c.db.execute("DELETE FROM fuzz_constraints");
      await c.db.execute("DELETE FROM fuzz_payments");
      await c.db.execute("DELETE FROM fuzz_order_items");
      await c.db.execute("DELETE FROM fuzz_orders");
      await c.db.execute("DELETE FROM fuzz_inventory");
      await c.db.execute("DELETE FROM fuzz_rel_products");
      await c.db.execute("DELETE FROM fuzz_rel_users");
      await c.db.execute("DELETE FROM fuzz_indexed");
      await c.db.execute("DELETE FROM fuzz_trigger_audit");
      await c.db.execute("DELETE FROM fuzz_edge_expectations");
      await c.db.execute("DELETE FROM fuzz_edge_payloads");
      await c.db.execute("DELETE FROM fuzz_transfer_events");
      await c.db.execute("DELETE FROM fuzz_accounts");
      await c.db.execute("DELETE FROM fuzz_item_events");
      await c.db.execute("DELETE FROM fuzz_items");
      await ensureAccounts(c.db);
      return await validate(c.db);
    },
    runPhase: async (c, input) => {
      const mode2 = input.mode ?? "balanced";
      const iterations = Math.max(1, Math.floor(input.iterations));
      const keySpace = Math.max(
        1,
        Math.floor(input.keySpace ?? DEFAULT_KEY_SPACE)
      );
      const maxPayloadBytes = Math.max(
        1,
        Math.floor(input.maxPayloadBytes ?? DEFAULT_MAX_PAYLOAD_BYTES)
      );
      const growthTargetBytes = Math.max(
        1,
        Math.floor(
          input.growthTargetBytes ?? DEFAULT_GROWTH_TARGET_BYTES
        )
      );
      const rng = makeRng(`${input.seed}:${input.phase}:${mode2}`);
      const ops = {};
      let stage = "ensureAccounts";
      try {
        await ensureAccounts(c.db);
        for (let i = 0; i < iterations; i += 1) {
          const kind = chooseKind(mode2, rng);
          ops[kind] = (ops[kind] ?? 0) + 1;
          stage = `base:${kind}:iteration:${i}`;
          if (kind === "transfer") {
            const fromIndex = intBetween(rng, 0, ACCOUNT_COUNT - 1);
            let toIndex = intBetween(rng, 0, ACCOUNT_COUNT - 1);
            if (toIndex === fromIndex)
              toIndex = (toIndex + 1) % ACCOUNT_COUNT;
            const fromAccount = `acct-${fromIndex}`;
            const toAccount = `acct-${toIndex}`;
            try {
              await applyTransfer(c.db, {
                phase: input.phase,
                localIndex: i,
                fromAccount,
                toAccount,
                amount: intBetween(rng, 1, 500)
              });
            } catch (error) {
              throw new Error(
                `base operation transfer failed at iteration ${i} from ${fromAccount} to ${toAccount}`,
                { cause: error }
              );
            }
          } else if (kind === "hot") {
            const itemKey = `hot-${intBetween(rng, 0, 3)}`;
            const updates = intBetween(
              rng,
              2,
              mode2 === "hot" ? 12 : 5
            );
            try {
              await applyHotUpdates(c.db, {
                seed: input.seed,
                phase: input.phase,
                localIndex: i,
                itemKey,
                updates,
                payloadBytes: intBetween(
                  rng,
                  1,
                  maxPayloadBytes
                )
              });
            } catch (error) {
              throw new Error(
                `base operation hot failed at iteration ${i} for ${itemKey} with ${updates} updates`,
                { cause: error }
              );
            }
          } else {
            const itemKey = mode2 === "hot" && rng() < 0.6 ? `hot-${intBetween(rng, 0, 3)}` : `item-${intBetween(rng, 0, keySpace - 1)}`;
            const payloadBytes = mode2 === "payloads" ? intBetween(
              rng,
              Math.min(256, maxPayloadBytes),
              maxPayloadBytes
            ) : intBetween(rng, 1, maxPayloadBytes);
            try {
              await applyItemOperation(c.db, {
                seed: input.seed,
                phase: input.phase,
                localIndex: i,
                kind,
                itemKey,
                payloadBytes
              });
            } catch (error) {
              throw new Error(
                `base operation ${kind} failed at iteration ${i} for ${JSON.stringify(itemKey)} with payloadBytes ${payloadBytes}`,
                { cause: error }
              );
            }
          }
        }
        stage = "deep-scenarios";
        await applyDeepScenarios(c.db, {
          seed: input.seed,
          phase: input.phase,
          mode: mode2,
          iterations,
          rng,
          maxPayloadBytes,
          growthTargetBytes,
          ops
        });
        stage = "validate";
        return {
          seed: input.seed,
          phase: input.phase,
          mode: mode2,
          iterations,
          ops,
          validation: await validate(c.db)
        };
      } catch (error) {
        const detail = error instanceof Error ? error.message : typeof error === "string" ? error : String(error);
        throw new Error(
          `runPhase failed during ${stage} for mode ${mode2} phase ${input.phase} seed ${input.seed}: ${detail}`,
          { cause: error }
        );
      }
    },
    validate: async (c) => {
      await ensureAccounts(c.db);
      return await validate(c.db);
    },
    debugItemMismatches: async (c, limit) => {
      await ensureAccounts(c.db);
      return await debugItemMismatches(c.db, limit ?? 5);
    },
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    }
  }
});

// src/actors/testing/sigterm-sleep-probe.ts
import {
  actor as actor43
} from "rivetkit";
import { db as db9 } from "rivetkit/db";
var DEFAULT_ON_SLEEP_DURATION_MS = 5e3;
var DEFAULT_ON_SLEEP_TICK_MS = 1e3;
var SLEEP_TIMEOUT_MS = 10 * 60 * 1e3;
var SLEEP_GRACE_PERIOD_MS = 30 * 60 * 1e3;
var ACTOR_STOPPED_CLOSE_CODE = 1e3;
var ACTOR_STOPPED_CLOSE_REASON = "actor stopped";
function sleep5(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, ms));
}
function formatError(error) {
  if (error instanceof Error) return error.stack ?? error.message;
  return String(error);
}
var sigtermSleepProbe = actor43({
  state: {
    label: "unprepared",
    wakeCount: 0,
    sleepCount: 0,
    onSleepDurationMs: DEFAULT_ON_SLEEP_DURATION_MS,
    onSleepTickMs: DEFAULT_ON_SLEEP_TICK_MS,
    connectionCount: 0,
    messageCount: 0,
    onSleepStartedAt: null,
    onSleepAsyncFinishedAt: null,
    onSleepFinishedAt: null,
    onSleepLastError: null
  },
  createVars: () => ({
    websockets: /* @__PURE__ */ new Set()
  }),
  db: db9({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS sigterm_sleep_log (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					event TEXT NOT NULL,
					sleep_count INTEGER NOT NULL,
					detail TEXT,
					created_at INTEGER NOT NULL
				)
			`);
    }
  }),
  onWebSocket: (c, websocket) => {
    c.vars.websockets.add(websocket);
    c.state.connectionCount += 1;
    const connectionId = crypto.randomUUID();
    c.log.info({
      msg: "sigterm sleep probe websocket connected",
      label: c.state.label,
      connectionId,
      connectionCount: c.state.connectionCount
    });
    websocket.send(
      JSON.stringify({
        type: "welcome",
        connectionId,
        label: c.state.label,
        connectionCount: c.state.connectionCount
      })
    );
    websocket.addEventListener("message", (event19) => {
      c.state.messageCount += 1;
      const data = event19.data;
      if (typeof data !== "string") return;
      try {
        const parsed = JSON.parse(data);
        if (parsed.type === "ping") {
          websocket.send(
            JSON.stringify({
              type: "pong",
              connectionId,
              messageCount: c.state.messageCount,
              timestamp: Date.now()
            })
          );
          return;
        }
      } catch {
      }
      websocket.send(
        JSON.stringify({
          type: "echo",
          connectionId,
          received: data,
          messageCount: c.state.messageCount,
          timestamp: Date.now()
        })
      );
    });
    websocket.addEventListener("close", (event19) => {
      c.vars.websockets.delete(websocket);
      c.state.connectionCount -= 1;
      c.log.info({
        msg: "sigterm sleep probe websocket closed",
        label: c.state.label,
        connectionId,
        connectionCount: c.state.connectionCount,
        code: event19.code,
        reason: event19.reason
      });
    });
  },
  onWake: async (c) => {
    c.state.wakeCount += 1;
    c.log.info({
      msg: "sigterm sleep probe onWake",
      label: c.state.label,
      wakeCount: c.state.wakeCount,
      sleepCount: c.state.sleepCount
    });
    await c.db.execute(
      "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
      "wake",
      c.state.sleepCount,
      `wake-${c.state.wakeCount}`,
      Date.now()
    );
  },
  onSleep: async (c) => {
    const sleepCount = c.state.sleepCount + 1;
    const startedAt = Date.now();
    c.state.sleepCount = sleepCount;
    c.state.onSleepStartedAt = startedAt;
    c.state.onSleepAsyncFinishedAt = null;
    c.state.onSleepFinishedAt = null;
    c.state.onSleepLastError = null;
    c.log.info({
      msg: "sigterm sleep probe onSleep start",
      label: c.state.label,
      sleepCount,
      onSleepDurationMs: c.state.onSleepDurationMs,
      onSleepTickMs: c.state.onSleepTickMs
    });
    try {
      for (const websocket of c.vars.websockets) {
        if (websocket.readyState !== 1) continue;
        websocket.send(
          JSON.stringify({
            type: "onSleepStarted",
            sleepCount,
            onSleepDurationMs: c.state.onSleepDurationMs,
            onSleepTickMs: c.state.onSleepTickMs,
            timestamp: startedAt
          })
        );
      }
      await c.db.execute(
        "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
        "on-sleep-start",
        sleepCount,
        c.state.label,
        startedAt
      );
      const deadline = startedAt + c.state.onSleepDurationMs;
      let tickIndex = 0;
      while (Date.now() < deadline) {
        const waitMs = Math.min(
          c.state.onSleepTickMs,
          Math.max(0, deadline - Date.now())
        );
        if (waitMs > 0) await sleep5(waitMs);
        tickIndex += 1;
        const tickAt = Date.now();
        const detail = `tick=${tickIndex} elapsed-ms=${tickAt - startedAt}`;
        await c.db.execute(
          "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
          "on-sleep-tick",
          sleepCount,
          detail,
          tickAt
        );
        c.log.info({
          msg: "sigterm sleep probe onSleep tick",
          label: c.state.label,
          sleepCount,
          tickIndex,
          elapsedMs: tickAt - startedAt
        });
        for (const websocket of c.vars.websockets) {
          if (websocket.readyState !== 1) continue;
          websocket.send(
            JSON.stringify({
              type: "onSleepTick",
              sleepCount,
              tickIndex,
              elapsedMs: tickAt - startedAt,
              timestamp: tickAt
            })
          );
        }
      }
      const asyncFinishedAt = Date.now();
      c.state.onSleepAsyncFinishedAt = asyncFinishedAt;
      await c.db.execute(
        "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
        "on-sleep-after-await",
        sleepCount,
        `delay-ms=${asyncFinishedAt - startedAt}`,
        asyncFinishedAt
      );
      const finishedAt = Date.now();
      c.state.onSleepFinishedAt = finishedAt;
      await c.db.execute(
        "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
        "on-sleep-finish",
        sleepCount,
        c.state.label,
        finishedAt
      );
      for (const websocket of c.vars.websockets) {
        if (websocket.readyState !== 1) continue;
        websocket.send(
          JSON.stringify({
            type: "onSleepFinished",
            sleepCount,
            elapsedMs: finishedAt - startedAt,
            timestamp: finishedAt
          })
        );
        websocket.close(
          ACTOR_STOPPED_CLOSE_CODE,
          ACTOR_STOPPED_CLOSE_REASON
        );
      }
      c.log.info({
        msg: "sigterm sleep probe onSleep finish",
        label: c.state.label,
        sleepCount,
        elapsedMs: finishedAt - startedAt
      });
    } catch (error) {
      const message = formatError(error);
      c.state.onSleepLastError = message;
      c.log.error({
        msg: "sigterm sleep probe onSleep error",
        label: c.state.label,
        sleepCount,
        error: message
      });
      throw error;
    }
  },
  actions: {
    prepare: async (c, label = `sigterm-sleep-probe-${Date.now()}`, onSleepDurationMs = DEFAULT_ON_SLEEP_DURATION_MS, onSleepTickMs = DEFAULT_ON_SLEEP_TICK_MS) => {
      if (!Number.isFinite(onSleepDurationMs) || onSleepDurationMs < 0) {
        throw new Error(
          "onSleepDurationMs must be a finite non-negative number"
        );
      }
      if (!Number.isFinite(onSleepTickMs) || onSleepTickMs <= 0) {
        throw new Error(
          "onSleepTickMs must be a finite positive number"
        );
      }
      c.state.label = label;
      c.state.onSleepDurationMs = onSleepDurationMs;
      c.state.onSleepTickMs = onSleepTickMs;
      await c.db.execute(
        "INSERT INTO sigterm_sleep_log (event, sleep_count, detail, created_at) VALUES (?, ?, ?, ?)",
        "prepared",
        c.state.sleepCount,
        label,
        Date.now()
      );
      return {
        label: c.state.label,
        onSleepDurationMs: c.state.onSleepDurationMs,
        onSleepTickMs: c.state.onSleepTickMs,
        wakeCount: c.state.wakeCount,
        sleepCount: c.state.sleepCount,
        connectionCount: c.state.connectionCount,
        messageCount: c.state.messageCount
      };
    },
    getProof: async (c) => {
      const rows = await c.db.execute("SELECT * FROM sigterm_sleep_log ORDER BY id");
      return {
        state: {
          label: c.state.label,
          wakeCount: c.state.wakeCount,
          sleepCount: c.state.sleepCount,
          onSleepDurationMs: c.state.onSleepDurationMs,
          onSleepTickMs: c.state.onSleepTickMs,
          connectionCount: c.state.connectionCount,
          messageCount: c.state.messageCount,
          onSleepStartedAt: c.state.onSleepStartedAt,
          onSleepAsyncFinishedAt: c.state.onSleepAsyncFinishedAt,
          onSleepFinishedAt: c.state.onSleepFinishedAt,
          onSleepLastError: c.state.onSleepLastError
        },
        rows
      };
    }
  },
  options: {
    canHibernateWebSocket: false,
    sleepTimeout: SLEEP_TIMEOUT_MS,
    sleepGracePeriod: SLEEP_GRACE_PERIOD_MS
  }
});

// src/actors/testing/sleep-close-fuzz.ts
import {
  actor as actor44
} from "rivetkit";
var sleepCloseFuzz = actor44({
  options: {
    canHibernateWebSocket: false
  },
  state: {
    connectionCount: 0,
    messageCount: 0
  },
  onWebSocket(c, websocket) {
    c.state.connectionCount += 1;
    const connectionId = crypto.randomUUID();
    websocket.send(
      JSON.stringify({
        type: "welcome",
        connectionId,
        connectionCount: c.state.connectionCount
      })
    );
    const interval = setInterval(() => {
      if (websocket.readyState !== 1) return;
      websocket.send(
        JSON.stringify({
          type: "tick",
          connectionId,
          timestamp: Date.now()
        })
      );
    }, 500);
    websocket.addEventListener("message", (event19) => {
      c.state.messageCount += 1;
      websocket.send(
        JSON.stringify({
          type: "echo",
          connectionId,
          received: event19.data
        })
      );
    });
    websocket.addEventListener("close", () => {
      clearInterval(interval);
      c.state.connectionCount -= 1;
    });
  },
  actions: {
    getStats(c) {
      return {
        connectionCount: c.state.connectionCount,
        messageCount: c.state.messageCount
      };
    }
  }
});

// src/actors/testing/slow-reconnect-actor.ts
import { actor as actor45, setup } from "rivetkit";
import { db as db10 } from "rivetkit/db";
var AsyncMutex2 = class {
  locked = false;
  waiters = [];
  async acquire() {
    if (!this.locked) {
      this.locked = true;
      return;
    }
    await new Promise((resolve2) => this.waiters.push(resolve2));
    this.locked = true;
  }
  release() {
    const next = this.waiters.shift();
    if (next) {
      next();
      return;
    }
    this.locked = false;
  }
};
function createDb(execute) {
  const mutex = new AsyncMutex2();
  let activeTransaction = null;
  const createTransactionDb = () => {
    const tx = Object.assign(
      (query, ...values) => execute(query, ...values),
      {
        withTransaction: async (fn) => fn(tx)
      }
    );
    return tx;
  };
  const queryWithMutex = async (query, ...values) => {
    if (activeTransaction) {
      return activeTransaction(query, ...values);
    }
    await mutex.acquire();
    try {
      return await execute(query, ...values);
    } finally {
      mutex.release();
    }
  };
  const sql = Object.assign(queryWithMutex, {
    withTransaction: async (fn) => {
      if (activeTransaction) {
        return fn(activeTransaction);
      }
      await mutex.acquire();
      const tx = createTransactionDb();
      try {
        await execute("BEGIN");
        activeTransaction = tx;
        try {
          const result = await fn(tx);
          activeTransaction = null;
          await execute("COMMIT");
          return result;
        } catch (error) {
          activeTransaction = null;
          await execute("ROLLBACK");
          throw error;
        }
      } finally {
        activeTransaction = null;
        mutex.release();
      }
    }
  });
  return sql;
}
var MESSAGE_COUNT2 = 84;
var MESSAGE_TOOL_REF_COUNT2 = 122;
var TOOL_CALL_COUNT2 = 61;
var EXECUTOR_TOOL_COUNT2 = 42;
var THREAD_EVENT_COUNT2 = 233;
var MESSAGE_CONTENT_BYTES2 = 10620;
var THREAD_EVENT_PAYLOAD_BYTES2 = 4036;
var TOOL_CALL_RESULT_BYTES2 = 10975;
var EXECUTOR_TOOL_SCHEMA_BYTES2 = 2235;
var slowReconnectActor = actor45({
  state: { runCount: 0 },
  db: db10({
    onMigrate: async (database) => {
      await createSlowReconnectSchema(database);
    }
  }),
  vars: { sql: null },
  onWebSocket: (c, ws) => {
    const sock = ws;
    if (sock.readyState === WebSocket.OPEN) {
      sock.send("pong");
    }
    ws.addEventListener("message", (event19) => {
      const promise = handleSlowReconnectWebSocketMessage(
        c,
        sock,
        event19.data
      );
      void c.keepAwake(promise);
    });
  },
  actions: {
    prepare: async (c) => {
      await createSlowReconnectSchema(c.db);
      return await seedSlowReconnectData(c.db);
    },
    reproReconnect: async (c, clientId) => {
      c.vars.sql ??= createSlowReconnectDb(c.db);
      c.state.runCount++;
      return await runReconnectRepro(
        c.vars.sql,
        clientId ?? `action-${c.state.runCount}`,
        0
      );
    },
    getRunCount: (c) => c.state.runCount,
    sleep: (c) => {
      c.sleep();
      return true;
    }
  }
});
async function handleSlowReconnectWebSocketMessage(c, sock, data) {
  if (data === "ping") {
    if (sock.readyState === WebSocket.OPEN) {
      sock.send("pong");
    }
    return;
  }
  let trigger = "unknown";
  try {
    const request = parseSlowReconnectRequest(data);
    trigger = request.type;
    c.vars.sql ??= createSlowReconnectDb(c.db);
    c.state.runCount++;
    if (request.type === "client_resume") {
      const startedAt = performance.now();
      const result2 = await runCatchupSnapshot2(
        c.vars.sql,
        request.version
      );
      sendJSON(sock, {
        type: "slow_reconnect_result",
        trigger: request.type,
        totalMs: Math.round(performance.now() - startedAt),
        results: [result2]
      });
      return;
    }
    const clientId = request.type === "executor_connect" ? request.clientId : request.clientId ?? `slow-reconnect-${c.state.runCount}`;
    const staggerHandleMs = request.type === "repro_reconnect" ? request.staggerHandleMs ?? 0 : 0;
    const result = await runReconnectRepro(
      c.vars.sql,
      clientId,
      staggerHandleMs
    );
    if (request.type === "executor_connect") {
      sendJSON(sock, {
        type: "executor_connected",
        executorId: clientId,
        registeredToolCount: EXECUTOR_TOOL_COUNT2,
        guidanceInventory: [],
        resumeBootstrap: true
      });
    }
    sendJSON(sock, {
      type: "slow_reconnect_result",
      trigger: request.type,
      ...result
    });
  } catch (error) {
    sendJSON(sock, {
      type: "slow_reconnect_error",
      trigger,
      error: error instanceof Error ? error.message : String(error)
    });
  }
}
function parseSlowReconnectRequest(data) {
  if (typeof data !== "string") {
    throw new Error("slowReconnectActor request must be a string");
  }
  const parsed = JSON.parse(data);
  if (!parsed || typeof parsed !== "object") {
    throw new Error("slowReconnectActor request must be an object");
  }
  const request = parsed;
  if (request.type === "client_resume") {
    return {
      type: "client_resume",
      version: numberField2(request, "version")
    };
  }
  if (request.type === "executor_connect") {
    const executorType = request.executorType;
    return {
      type: "executor_connect",
      clientId: stringField2(request, "clientId"),
      ...executorType === "local-client" || executorType === "sandbox" || executorType === "virtual" ? { executorType } : {}
    };
  }
  if (request.type === "repro_reconnect") {
    return {
      type: "repro_reconnect",
      ...typeof request.clientId === "string" ? { clientId: request.clientId } : {},
      ...typeof request.staggerHandleMs === "number" ? { staggerHandleMs: request.staggerHandleMs } : {}
    };
  }
  throw new Error(
    `Unknown slowReconnectActor request type: ${String(request.type)}`
  );
}
function stringField2(record, field) {
  const value = record[field];
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(
      `slowReconnectActor request ${field} must be a non-empty string`
    );
  }
  return value;
}
function numberField2(record, field) {
  const value = record[field];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(
      `slowReconnectActor request ${field} must be a finite number`
    );
  }
  return value;
}
function sendJSON(sock, message) {
  if (sock.readyState === WebSocket.OPEN) {
    sock.send(JSON.stringify(message));
  }
}
function createSlowReconnectDb(db17) {
  return createDb(
    async (query, ...values) => {
      const converted = values.map(
        (value) => typeof value === "boolean" ? value ? 1 : 0 : value
      );
      return await db17.execute(query, ...converted);
    }
  );
}
async function runReconnectRepro(sql, clientId, staggerHandleMs) {
  const startedAt = performance.now();
  const buildToolPlanContext = await runBuildToolPlanContext2(sql);
  const catchupSnapshot = await runCatchupSnapshot2(sql, 0);
  const recoverToolCalls = await runRecoverToolCalls2(sql);
  const handleExecutorConnect = await delay2(staggerHandleMs).then(
    () => runHandleExecutorConnect(sql, clientId)
  );
  const results = [
    handleExecutorConnect,
    buildToolPlanContext,
    catchupSnapshot,
    recoverToolCalls
  ];
  return {
    totalMs: Math.round(performance.now() - startedAt),
    results
  };
}
async function runHandleExecutorConnect(sql, clientId) {
  const startedAt = performance.now();
  const steps = [];
  const nextSeq = await sql.withTransaction(async (tx) => {
    const latestExecutor = await timedQuery2(
      tx,
      steps,
      "load-latest-executor-id",
      `SELECT executor_id FROM executor_tools ORDER BY updated_at DESC LIMIT 1`
    );
    const latestExecutorId = String(
      latestExecutor[0]?.executor_id ?? "seed-executor"
    );
    await timedQuery2(
      tx,
      steps,
      "select-cached-executor-tools",
      `SELECT tool_name, schema FROM executor_tools WHERE executor_id = ? ORDER BY tool_name ASC`,
      latestExecutorId
    );
    const executorType = await timedQuery2(
      tx,
      steps,
      "select-executor-type",
      `SELECT value FROM thread_meta_kv WHERE key = 'executor_type'`
    );
    if (!executorType[0]?.value) {
      await timedQuery2(
        tx,
        steps,
        "set-executor-type",
        `INSERT OR REPLACE INTO thread_meta_kv (key, value, updated_at) VALUES ('executor_type', ?, ?)`,
        "local-client",
        (/* @__PURE__ */ new Date()).toISOString()
      );
    }
    const sandboxIntent = await timedQuery2(
      tx,
      steps,
      "select-sandbox-intent",
      `SELECT value FROM thread_meta_kv WHERE key = 'sandbox_intent'`
    );
    if (hasPendingLaunch2(sandboxIntent[0]?.value)) {
      await timedQuery2(
        tx,
        steps,
        "clear-pending-launch",
        `UPDATE thread_meta_kv SET value = ?, updated_at = ? WHERE key = 'sandbox_intent'`,
        JSON.stringify({ spec: null, pendingLaunch: null }),
        (/* @__PURE__ */ new Date()).toISOString()
      );
    }
    const seqRows = await timedQuery2(
      tx,
      steps,
      "select-next-thread-event-seq",
      `SELECT COALESCE(MAX(seq), 0) + 1 AS seq FROM thread_events`
    );
    const seq = Number(seqRows[0]?.seq ?? 1);
    await timedQuery2(
      tx,
      steps,
      "insert-executor-connected-event",
      `INSERT INTO thread_events (seq, event_type, payload, created_at) VALUES (?, ?, ?, ?)`,
      seq,
      "executor_connected",
      JSON.stringify({
        type: "executor_connected",
        executorId: clientId
      }),
      (/* @__PURE__ */ new Date()).toISOString()
    );
    return seq;
  });
  steps.push({
    name: "transaction-total",
    durationMs: Math.round(performance.now() - startedAt),
    rowCount: nextSeq
  });
  return {
    name: "handle-executor-connect",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runBuildToolPlanContext2(sql) {
  const startedAt = performance.now();
  const steps = [];
  const latestExecutor = await timedQuery2(
    sql,
    steps,
    "load-latest-executor-id",
    `SELECT executor_id FROM executor_tools ORDER BY updated_at DESC LIMIT 1`
  );
  const latestExecutorId = String(
    latestExecutor[0]?.executor_id ?? "seed-executor"
  );
  await timedQuery2(
    sql,
    steps,
    "select-executor-tools",
    `SELECT tool_name, schema FROM executor_tools WHERE executor_id = ? ORDER BY tool_name ASC`,
    latestExecutorId
  );
  await timedQuery2(
    sql,
    steps,
    "count-uncancelled-top-level",
    `SELECT COUNT(*) as count FROM messages WHERE cancelled = 0 AND parent_tool_use_id IS NULL`
  );
  const unresolvedRows = await timedQuery2(
    sql,
    steps,
    "find-unresolved-assistant-message",
    `SELECT m.*
			FROM message_tool_refs AS tool_use
			JOIN messages AS m
				ON m.message_id = tool_use.assistant_message_id
			WHERE tool_use.block_type = 'tool_use'
				AND tool_use.cancelled = 0
				AND m.cancelled = 0
				AND m.role = 'assistant'
				AND m.parent_tool_use_id IS NULL
				AND NOT EXISTS (
					SELECT 1
					FROM message_tool_refs AS tool_result
					JOIN messages AS tool_result_message
						ON tool_result_message.message_id = tool_result.source_message_id
					WHERE tool_result.assistant_message_id = tool_use.assistant_message_id
						AND tool_result.block_type = 'tool_result'
						AND tool_result.cancelled = 0
						AND tool_result.tool_use_id = tool_use.tool_use_id
						AND tool_result_message.parent_tool_use_id IS NULL
				)
			GROUP BY m.message_id
			ORDER BY m.created_at DESC
			LIMIT 1`
  );
  const unresolvedMessageId = unresolvedRows[0]?.message_id;
  if (typeof unresolvedMessageId === "string") {
    await timedQuery2(
      sql,
      steps,
      "get-persisted-tool-result-ids",
      `SELECT tool_result.tool_use_id
				FROM message_tool_refs AS tool_result
				JOIN messages AS tool_result_message
					ON tool_result_message.message_id = tool_result.source_message_id
				WHERE tool_result.assistant_message_id = ?
					AND tool_result.block_type = 'tool_result'
					AND tool_result.cancelled = 0
					AND tool_result_message.parent_tool_use_id IS NULL`,
      unresolvedMessageId
    );
    await timedQuery2(
      sql,
      steps,
      "get-tool-calls-by-message-id",
      `SELECT * FROM tool_calls WHERE message_id = ?`,
      unresolvedMessageId
    );
  }
  await timedQuery2(
    sql,
    steps,
    "is-last-message-cancelled-assistant",
    `SELECT role, cancelled FROM messages
			WHERE parent_tool_use_id IS NULL
			ORDER BY created_at DESC
			LIMIT 1`
  );
  await timedQuery2(
    sql,
    steps,
    "get-last-uncancelled",
    `SELECT m.* FROM messages m
			WHERE m.cancelled = 0 AND m.parent_tool_use_id IS NULL
			ORDER BY m.created_at DESC
			LIMIT 1`
  );
  return {
    name: "build-tool-plan-context",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runCatchupSnapshot2(sql, version) {
  const startedAt = performance.now();
  const steps = [];
  await timedQuery2(
    sql,
    steps,
    "thread-events-list-since-version",
    `SELECT seq, event_type, payload, created_at FROM thread_events WHERE seq > ? ORDER BY seq ASC`,
    version
  );
  await timedQuery2(
    sql,
    steps,
    "environment-snapshot",
    `SELECT snapshot FROM environment_snapshot WHERE id = 1`
  );
  await timedQuery2(
    sql,
    steps,
    "thread-settings-snapshot",
    `SELECT settings FROM thread_settings_snapshot WHERE id = 1`
  );
  await timedQuery2(
    sql,
    steps,
    "retry-state",
    `SELECT * FROM retry_state WHERE id = 1`
  );
  await timedQuery2(
    sql,
    steps,
    "queued-messages",
    `SELECT * FROM queued_messages ORDER BY created_at ASC`
  );
  await timedQuery2(
    sql,
    steps,
    "executor-artifacts",
    `SELECT artifact_key, data_type, length(content_base64) AS bytes, tool_call_id, updated_at FROM executor_artifacts ORDER BY updated_at ASC`
  );
  await timedQuery2(
    sql,
    steps,
    "tool-approvals",
    `SELECT * FROM tool_approvals ORDER BY timestamp ASC`
  );
  await timedQuery2(
    sql,
    steps,
    "compaction-summaries",
    `SELECT cut_message_id, created_at FROM compaction_summaries ORDER BY created_at ASC`
  );
  await timedQuery2(
    sql,
    steps,
    "executor-status",
    `SELECT value FROM thread_meta_kv WHERE key = 'executor_status'`
  );
  steps.sort((a, b) => b.durationMs - a.durationMs);
  return {
    name: "catchup-snapshot",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function runRecoverToolCalls2(sql) {
  const startedAt = performance.now();
  const steps = [];
  await timedQuery2(
    sql,
    steps,
    "hydrate-tool-progress",
    `SELECT id, progress
			FROM tool_calls
			WHERE progress IS NOT NULL
				AND state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  await timedQuery2(
    sql,
    steps,
    "get-pending-tool-calls",
    `SELECT * FROM tool_calls
			WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')
			ORDER BY issued_at ASC`
  );
  await timedQuery2(
    sql,
    steps,
    "get-next-tool-expiry",
    `SELECT MIN(expires_at) AS expires_at
			FROM tool_calls
			WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  return {
    name: "recover-tool-calls",
    totalMs: Math.round(performance.now() - startedAt),
    steps
  };
}
async function timedQuery2(sql, steps, name, query, ...values) {
  const startedAt = performance.now();
  const rows = await sql(query, ...values);
  steps.push({
    name,
    durationMs: Math.round(performance.now() - startedAt),
    rowCount: rows.length
  });
  return rows;
}
function hasPendingLaunch2(value) {
  if (typeof value !== "string" || value.length === 0) {
    return false;
  }
  try {
    const parsed = JSON.parse(value);
    return parsed.pendingLaunch !== null && parsed.pendingLaunch !== void 0;
  } catch {
    return false;
  }
}
function delay2(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, Math.max(0, ms)));
}
async function createSlowReconnectSchema(database) {
  await database.execute(`CREATE TABLE IF NOT EXISTS executor_tools (
		executor_id TEXT NOT NULL,
		tool_name TEXT NOT NULL,
		schema TEXT NOT NULL,
		updated_at TEXT NOT NULL,
		PRIMARY KEY (executor_id, tool_name)
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_executor_tools_executor ON executor_tools(executor_id)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS thread_meta_kv (
		key TEXT PRIMARY KEY,
		value TEXT,
		updated_at TEXT NOT NULL
	)`);
  await database.execute(`CREATE TABLE IF NOT EXISTS thread_events (
		seq INTEGER PRIMARY KEY,
		event_type TEXT NOT NULL,
		payload TEXT NOT NULL,
		created_at TEXT NOT NULL DEFAULT (datetime('now'))
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_thread_events_seq ON thread_events(seq)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS message_added_events (
		message_id TEXT PRIMARY KEY,
		seq INTEGER NOT NULL UNIQUE
	)`);
  await database.execute(`CREATE TABLE IF NOT EXISTS messages (
		message_id TEXT PRIMARY KEY,
		role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'info')),
		content TEXT NOT NULL,
		meta TEXT,
		user_state TEXT,
		created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
		cancelled INTEGER NOT NULL DEFAULT 0,
		read_at TEXT,
		parent_tool_use_id TEXT,
		tool_result_for_message_id TEXT
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_role_cancelled_created_at ON messages(parent_tool_use_id, role, cancelled, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_cancelled_created_at ON messages(parent_tool_use_id, cancelled, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_parent_created_at ON messages(parent_tool_use_id, created_at DESC)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_messages_role_created_at ON messages(role, created_at)`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS message_tool_refs (
		source_message_id TEXT NOT NULL,
		assistant_message_id TEXT NOT NULL,
		tool_use_id TEXT NOT NULL,
		block_type TEXT NOT NULL CHECK(block_type IN ('tool_use', 'tool_result')),
		cancelled INTEGER NOT NULL DEFAULT 0,
		PRIMARY KEY (source_message_id, block_type, tool_use_id)
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_assistant_lookup ON message_tool_refs(assistant_message_id, block_type, cancelled, tool_use_id)`
  );
  await database.execute(
    `CREATE UNIQUE INDEX IF NOT EXISTS idx_message_tool_refs_live_tool_result ON message_tool_refs(assistant_message_id, tool_use_id) WHERE block_type = 'tool_result' AND cancelled = 0`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_source_message ON message_tool_refs(source_message_id)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_message_tool_refs_tool_use_lookup ON message_tool_refs(tool_use_id, assistant_message_id) WHERE block_type = 'tool_use' AND cancelled = 0`
  );
  await database.execute(`CREATE TABLE IF NOT EXISTS tool_calls (
		id TEXT PRIMARY KEY,
		provider_tool_use_id TEXT NOT NULL,
		tool_name TEXT NOT NULL,
		args TEXT NOT NULL,
		executor_id TEXT,
		message_id TEXT NOT NULL,
		issued_at TEXT NOT NULL,
		expires_at TEXT,
		state TEXT NOT NULL CHECK(state IN ('queued', 'pending_reconnect', 'pending_ack', 'running', 'completed', 'expired', 'revoked')),
		result TEXT,
		progress TEXT,
		completed_at TEXT
	)`);
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_message_id ON tool_calls(message_id)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_state ON tool_calls(state)`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_calls_expires_at ON tool_calls(expires_at) WHERE state IN ('queued', 'pending_reconnect', 'pending_ack', 'running')`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS environment_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), snapshot TEXT NOT NULL, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS thread_settings_snapshot (id INTEGER PRIMARY KEY CHECK (id = 1), settings TEXT NOT NULL, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS retry_state (id INTEGER PRIMARY KEY CHECK (id = 1), attempt INTEGER NOT NULL DEFAULT 0, scheduled_at INTEGER NOT NULL, reason TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS queued_messages (message_id TEXT PRIMARY KEY, content TEXT NOT NULL, user_state TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')), steer INTEGER NOT NULL DEFAULT 0, user_meta TEXT)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS executor_artifacts (artifact_key TEXT PRIMARY KEY, data_type TEXT NOT NULL, content_base64 TEXT NOT NULL, tool_call_id TEXT, updated_at TEXT NOT NULL)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS tool_approvals (id TEXT PRIMARY KEY, tool_call_id TEXT NOT NULL UNIQUE, tool_name TEXT NOT NULL, args TEXT NOT NULL, reason TEXT, to_allow TEXT, context TEXT NOT NULL CHECK(context IN ('thread', 'subagent')), subagent_tool_name TEXT, parent_tool_call_id TEXT, timestamp INTEGER NOT NULL, matched_rule TEXT, rule_source TEXT CHECK(rule_source IN ('user', 'built-in')))`
  );
  await database.execute(
    `CREATE INDEX IF NOT EXISTS idx_tool_approvals_timestamp ON tool_approvals(timestamp)`
  );
  await database.execute(
    `CREATE TABLE IF NOT EXISTS compaction_summaries (summary_id TEXT PRIMARY KEY, summary_text TEXT NOT NULL, cut_message_id TEXT NOT NULL, created_at TEXT NOT NULL)`
  );
}
async function seedSlowReconnectData(database) {
  const existing = await database.execute(
    `SELECT COUNT(*) AS count FROM messages`
  );
  if (Number(existing[0]?.count ?? 0) > 0) {
    const [toolCalls] = await database.execute(
      `SELECT COUNT(*) AS count FROM tool_calls`
    );
    const [threadEvents] = await database.execute(
      `SELECT COUNT(*) AS count FROM thread_events`
    );
    return {
      seeded: false,
      messages: Number(existing[0]?.count ?? 0),
      toolCalls: Number(toolCalls?.count ?? 0),
      threadEvents: Number(threadEvents?.count ?? 0)
    };
  }
  const vary = (base, min = 1) => Math.max(min, Math.round(base * (0.25 + Math.random() * 3.75)));
  const messageCount = vary(MESSAGE_COUNT2, 2);
  const messageToolRefCount = vary(MESSAGE_TOOL_REF_COUNT2, 2);
  const toolCallCount = vary(TOOL_CALL_COUNT2);
  const executorToolCount = vary(EXECUTOR_TOOL_COUNT2);
  const threadEventCount = vary(THREAD_EVENT_COUNT2);
  const assistantSpan = Math.max(1, Math.floor(messageCount / 2));
  const now = (/* @__PURE__ */ new Date("2026-05-16T03:58:18.661Z")).getTime();
  const text2 = (size) => "x".repeat(size);
  const isoAt = (index) => new Date(now + index * 1e3).toISOString();
  await batchInsert2(
    database,
    `INSERT INTO thread_meta_kv (key, value, updated_at)`,
    [
      ["executor_type", "local-client", isoAt(0)],
      [
        "sandbox_intent",
        JSON.stringify({ spec: null, pendingLaunch: null }),
        isoAt(0)
      ],
      [
        "executor_status",
        JSON.stringify({ available: true, message: "ready" }),
        isoAt(0)
      ]
    ]
  );
  const messageRows = [];
  for (let index = 1; index <= messageCount; index++) {
    const role = index % 2 === 0 ? "assistant" : "user";
    messageRows.push([
      messageId2(index),
      role,
      text2(MESSAGE_CONTENT_BYTES2),
      null,
      null,
      isoAt(index),
      0,
      null,
      null,
      null
    ]);
  }
  await batchInsert2(
    database,
    `INSERT INTO messages (message_id, role, content, meta, user_state, created_at, cancelled, read_at, parent_tool_use_id, tool_result_for_message_id)`,
    messageRows,
    20
  );
  const messageToolRefRows = [];
  for (let index = 0; index < messageToolRefCount / 2; index++) {
    const assistantIndex = 2 + index % assistantSpan * 2;
    const sourceIndex = Math.max(1, assistantIndex - 1);
    const resultIndex = Math.min(messageCount, assistantIndex + 1);
    const toolUseId = toolUseID2(index + 1);
    messageToolRefRows.push([
      messageId2(sourceIndex),
      messageId2(assistantIndex),
      toolUseId,
      "tool_use",
      0
    ]);
    messageToolRefRows.push([
      messageId2(resultIndex),
      messageId2(assistantIndex),
      toolUseId,
      "tool_result",
      0
    ]);
  }
  await batchInsert2(
    database,
    `INSERT INTO message_tool_refs (source_message_id, assistant_message_id, tool_use_id, block_type, cancelled)`,
    messageToolRefRows,
    50
  );
  const toolCallRows = [];
  for (let index = 1; index <= toolCallCount; index++) {
    const assistantIndex = 2 + (index - 1) % assistantSpan * 2;
    toolCallRows.push([
      toolUseID2(index),
      `provider-${index}`,
      `tool_${index % 21}`,
      JSON.stringify({ path: `/tmp/file-${index}` }),
      "seed-executor",
      messageId2(assistantIndex),
      isoAt(index),
      null,
      "completed",
      JSON.stringify({
        ok: true,
        run: { status: "done", result: text2(TOOL_CALL_RESULT_BYTES2) }
      }),
      null,
      isoAt(index + 100)
    ]);
  }
  await batchInsert2(
    database,
    `INSERT INTO tool_calls (id, provider_tool_use_id, tool_name, args, executor_id, message_id, issued_at, expires_at, state, result, progress, completed_at)`,
    toolCallRows,
    20
  );
  const executorToolRows = [];
  for (let index = 1; index <= executorToolCount; index++) {
    const schema = JSON.stringify({
      name: `tool_${index}`,
      description: text2(EXECUTOR_TOOL_SCHEMA_BYTES2),
      input_schema: { type: "object", properties: {} }
    });
    executorToolRows.push([
      "seed-executor",
      `tool_${index}`,
      schema,
      isoAt(index)
    ]);
  }
  await batchInsert2(
    database,
    `INSERT INTO executor_tools (executor_id, tool_name, schema, updated_at)`,
    executorToolRows,
    42
  );
  const threadEventRows = [];
  for (let index = 1; index <= threadEventCount; index++) {
    threadEventRows.push([
      index,
      index % 3 === 0 ? "message_added" : "agent_state_changed",
      JSON.stringify({
        type: "seed_event",
        body: text2(THREAD_EVENT_PAYLOAD_BYTES2)
      }),
      isoAt(index)
    ]);
  }
  await batchInsert2(
    database,
    `INSERT INTO thread_events (seq, event_type, payload, created_at)`,
    threadEventRows,
    25
  );
  const messageAddedRows = [];
  for (let index = 1; index <= messageCount; index++) {
    messageAddedRows.push([messageId2(index), index]);
  }
  await batchInsert2(
    database,
    `INSERT INTO message_added_events (message_id, seq)`,
    messageAddedRows,
    50
  );
  await database.execute(
    `INSERT INTO environment_snapshot (id, snapshot, updated_at) VALUES (1, ?, ?)`,
    JSON.stringify({ cwd: "/workspace", body: text2(3620) }),
    isoAt(0)
  );
  await database.execute(
    `INSERT INTO thread_settings_snapshot (id, settings, updated_at) VALUES (1, ?, ?)`,
    JSON.stringify({ maxTokens: 2e4, body: text2(55) }),
    isoAt(0)
  );
  return {
    seeded: true,
    messages: messageCount,
    toolCalls: toolCallCount,
    threadEvents: threadEventCount
  };
}
var MAX_BATCH_BIND_BYTES = 120 * 1024;
function bindParamBytes(value) {
  if (value === null || value === void 0) {
    return 0;
  }
  if (typeof value === "string") {
    return Buffer.byteLength(value, "utf8");
  }
  if (typeof value === "number") {
    return 8;
  }
  if (value instanceof Uint8Array) {
    return value.byteLength;
  }
  return Buffer.byteLength(String(value), "utf8");
}
async function batchInsert2(database, insertPrefix, rows, batchSize = 100) {
  if (rows.length === 0) {
    return;
  }
  const columnCount = rows[0]?.length ?? 0;
  if (columnCount === 0) {
    return;
  }
  const rowPlaceholder = `(${"?,".repeat(columnCount).slice(0, -1)})`;
  const rowBytes = rows.map(
    (row) => row.reduce((sum, value) => sum + bindParamBytes(value), 0)
  );
  let index = 0;
  while (index < rows.length) {
    const chunk = [];
    let chunkBytes = 0;
    while (index < rows.length && chunk.length < batchSize && // Always take at least one row so an oversized single row still runs
    // (and surfaces the engine error) rather than looping forever.
    (chunk.length === 0 || chunkBytes + rowBytes[index] <= MAX_BATCH_BIND_BYTES)) {
      chunk.push(rows[index]);
      chunkBytes += rowBytes[index];
      index++;
    }
    const values = chunk.map(() => rowPlaceholder).join(",");
    const bindings = chunk.flat();
    await database.execute(`${insertPrefix} VALUES ${values}`, ...bindings);
  }
}
function messageId2(index) {
  return `M-${String(index).padStart(22, "0")}`;
}
function toolUseID2(index) {
  return `toolu_${String(index).padStart(22, "0")}`;
}
var registry = setup({
  use: { slowReconnectActor },
  maxIncomingMessageSize: 5 * 1024 * 1024,
  maxOutgoingMessageSize: 5 * 1024 * 1024
});

// src/actors/testing/sqlite-cold-start-bench.ts
import { randomBytes } from "crypto";
import { actor as actor46 } from "rivetkit";
import { db as db11 } from "rivetkit/db";
var DEFAULT_TARGET_BYTES = 50 * 1024 * 1024;
var DEFAULT_ROW_BYTES2 = 16 * 1024;
var DEFAULT_BATCH_ROWS2 = 8;
var DEFAULT_TRANSACTION_BYTES = 64 * 1024;
var READ_BATCH_ROWS = 64;
var REVERSE_PROBE_ROWS = 32 * 1024;
var PAYLOAD_TABLE = "cold_start_payload";
var REVERSE_PROBE_TABLE = "cold_start_reverse_probe";
function positiveInteger2(value, fallback, name) {
  const resolved = value ?? fallback;
  if (!Number.isInteger(resolved) || resolved < 1) {
    throw new Error(`${name} must be a positive integer`);
  }
  return resolved;
}
function randomAsciiString(bytes) {
  return randomBytes(Math.ceil(bytes / 2)).toString("hex").slice(0, bytes);
}
async function readPayloads(database, direction = "forward") {
  const t0 = performance.now();
  const [bounds] = await database.execute(
    `
			SELECT
				MIN(id) AS min_id,
				MAX(id) AS max_id,
				COUNT(*) AS rows,
				0 AS bytes,
				0 AS expected_bytes
			FROM ${PAYLOAD_TABLE}
		`
  );
  if (!bounds) throw new Error("read query returned no rows");
  let rows = 0;
  let bytes = 0;
  let expectedBytes = 0;
  let chunks = 0;
  const minId = bounds.min_id ?? 0;
  const maxId = bounds.max_id ?? 0;
  if (direction === "backward") {
    const [probeBounds] = await database.execute(
      `
				SELECT
					MIN(id) AS min_id,
					MAX(id) AS max_id,
					COUNT(*) AS rows,
					0 AS bytes,
					0 AS expected_bytes
				FROM ${REVERSE_PROBE_TABLE}
			`
    );
    if (!probeBounds)
      throw new Error("reverse probe query returned no rows");
    const probeMinId = probeBounds.min_id ?? 0;
    const probeMaxId = probeBounds.max_id ?? 0;
    for (let upperId = probeMaxId; upperId >= probeMinId && upperId > 0; upperId -= READ_BATCH_ROWS) {
      const lowerId = Math.max(probeMinId, upperId - READ_BATCH_ROWS + 1);
      const chunkRows = await database.execute(
        `
					SELECT
						marker AS bytes,
						marker AS expected_bytes
					FROM ${REVERSE_PROBE_TABLE}
					WHERE id BETWEEN ? AND ?
					ORDER BY id DESC
				`,
        lowerId,
        upperId
      );
      for (const row of chunkRows) {
        rows += 1;
        bytes += row.bytes;
        expectedBytes += row.expected_bytes;
      }
      chunks += 1;
    }
    return {
      ms: performance.now() - t0,
      ops: rows,
      rows,
      bytes,
      expectedBytes,
      chunks,
      readBatchRows: READ_BATCH_ROWS,
      direction
    };
  }
  for (let lowerId = minId; lowerId <= maxId; lowerId += READ_BATCH_ROWS) {
    const upperId = lowerId + READ_BATCH_ROWS - 1;
    const [chunk] = await database.execute(
      `
				SELECT
					COUNT(*) AS rows,
					COALESCE(SUM(length(payload)), 0) AS bytes,
					COALESCE(SUM(payload_bytes), 0) AS expected_bytes
				FROM ${PAYLOAD_TABLE}
				WHERE id BETWEEN ? AND ?
			`,
      lowerId,
      upperId
    );
    if (!chunk) throw new Error("chunked read query returned no rows");
    rows += chunk.rows;
    bytes += chunk.bytes;
    expectedBytes += chunk.expected_bytes;
    chunks += 1;
  }
  return {
    ms: performance.now() - t0,
    ops: rows,
    rows,
    bytes,
    expectedBytes,
    chunks,
    readBatchRows: READ_BATCH_ROWS
  };
}
var sqliteColdStartBench = actor46({
  options: {
    actionTimeout: 6e5
  },
  db: db11({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS cold_start_payload (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					payload TEXT NOT NULL,
					payload_bytes INTEGER NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(`
				CREATE TABLE IF NOT EXISTS cold_start_reverse_probe (
					id INTEGER PRIMARY KEY,
					marker INTEGER NOT NULL
				)
			`);
    }
  }),
  actions: {
    reset: async (c) => {
      await c.db.execute(`DELETE FROM ${PAYLOAD_TABLE}`);
      await c.db.execute(`DELETE FROM ${REVERSE_PROBE_TABLE}`);
      return { ok: true };
    },
    writeRandomStrings: async (c, input = {}) => {
      const targetBytes = positiveInteger2(
        input.targetBytes,
        DEFAULT_TARGET_BYTES,
        "targetBytes"
      );
      const rowBytes = positiveInteger2(
        input.rowBytes,
        DEFAULT_ROW_BYTES2,
        "rowBytes"
      );
      const batchRows = positiveInteger2(
        input.batchRows,
        DEFAULT_BATCH_ROWS2,
        "batchRows"
      );
      const transactionBytes = positiveInteger2(
        input.transactionBytes,
        DEFAULT_TRANSACTION_BYTES,
        "transactionBytes"
      );
      const createdAt = Date.now();
      let remainingBytes = targetBytes;
      let rows = 0;
      let transactions = 0;
      let randomStringMs = 0;
      let sqliteInsertMs = 0;
      let commitMs = 0;
      let inTransaction = false;
      const wallT0 = performance.now();
      try {
        while (remainingBytes > 0) {
          let transactionRemainingBytes = Math.min(
            transactionBytes,
            remainingBytes
          );
          await c.db.execute("BEGIN");
          inTransaction = true;
          transactions += 1;
          while (transactionRemainingBytes > 0) {
            const placeholders = [];
            const args = [];
            const generateT0 = performance.now();
            for (let batchIndex = 0; batchIndex < batchRows && transactionRemainingBytes > 0 && remainingBytes > 0; batchIndex += 1) {
              const payloadBytes = Math.min(
                rowBytes,
                transactionRemainingBytes,
                remainingBytes
              );
              placeholders.push("(?, ?, ?)");
              args.push(
                randomAsciiString(payloadBytes),
                payloadBytes,
                createdAt + rows
              );
              transactionRemainingBytes -= payloadBytes;
              remainingBytes -= payloadBytes;
              rows += 1;
            }
            randomStringMs += performance.now() - generateT0;
            const insertT0 = performance.now();
            await c.db.execute(
              `INSERT INTO ${PAYLOAD_TABLE} (payload, payload_bytes, created_at) VALUES ${placeholders.join(", ")}`,
              ...args
            );
            sqliteInsertMs += performance.now() - insertT0;
          }
          const commitT0 = performance.now();
          await c.db.execute("COMMIT");
          commitMs += performance.now() - commitT0;
          inTransaction = false;
        }
        await c.db.execute("BEGIN");
        inTransaction = true;
        for (let lowerId = 1; lowerId <= REVERSE_PROBE_ROWS; lowerId += 256) {
          const upperId = Math.min(REVERSE_PROBE_ROWS, lowerId + 255);
          const placeholders = [];
          const args = [];
          for (let id = lowerId; id <= upperId; id += 1) {
            placeholders.push("(?, ?)");
            args.push(id, 1);
          }
          const insertT0 = performance.now();
          await c.db.execute(
            `INSERT INTO ${REVERSE_PROBE_TABLE} (id, marker) VALUES ${placeholders.join(", ")}`,
            ...args
          );
          sqliteInsertMs += performance.now() - insertT0;
        }
        const reverseCommitT0 = performance.now();
        await c.db.execute("COMMIT");
        commitMs += performance.now() - reverseCommitT0;
        inTransaction = false;
        return {
          ms: sqliteInsertMs + commitMs,
          writeWallMs: performance.now() - wallT0,
          randomStringMs,
          sqliteInsertMs,
          commitMs,
          ops: rows,
          rows,
          transactions,
          bytes: targetBytes,
          rowBytes,
          batchRows,
          transactionBytes,
          reverseProbeRows: REVERSE_PROBE_ROWS
        };
      } catch (err) {
        if (inTransaction) {
          await c.db.execute("ROLLBACK");
        }
        throw err;
      }
    },
    readAll: async (c) => {
      return readPayloads(c.db);
    },
    readAllReverse: async (c) => {
      return readPayloads(c.db, "backward");
    },
    wakeSqlite: async (c) => {
      const t0 = performance.now();
      const [row] = await c.db.execute(
        `SELECT COUNT(*) AS rows FROM ${PAYLOAD_TABLE} WHERE id = -1`
      );
      return {
        ms: performance.now() - t0,
        rows: row?.rows ?? 0
      };
    },
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    }
  }
});

// src/actors/testing/sqlite-memory-pressure.ts
import { actor as actor47 } from "rivetkit";
import { db as db12 } from "rivetkit/db";
var DEFAULT_INSERT_ROWS = 128;
var DEFAULT_ROW_BYTES3 = 16 * 1024;
var DEFAULT_SCAN_ROWS = 512;
var INSERT_BATCH_ROWS = 32;
function finiteInt2(value, fallback) {
  if (value === void 0) return fallback;
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`expected a non-negative finite number, got ${value}`);
  }
  return Math.floor(value);
}
function copyNativeMetrics(metrics) {
  if (!metrics) return null;
  const raw = metrics;
  const numberField3 = (camel, snake) => Number(raw[camel] ?? raw[snake] ?? 0);
  return {
    requestBuildNs: numberField3("requestBuildNs", "request_build_ns"),
    serializeNs: numberField3("serializeNs", "serialize_ns"),
    transportNs: numberField3("transportNs", "transport_ns"),
    stateUpdateNs: numberField3("stateUpdateNs", "state_update_ns"),
    totalNs: numberField3("totalNs", "total_ns"),
    commitCount: numberField3("commitCount", "commit_count"),
    pageCacheEntries: numberField3("pageCacheEntries", "page_cache_entries"),
    pageCacheWeightedSize: numberField3(
      "pageCacheWeightedSize",
      "page_cache_weighted_size"
    ),
    pageCacheCapacityPages: numberField3(
      "pageCacheCapacityPages",
      "page_cache_capacity_pages"
    ),
    writeBufferDirtyPages: numberField3(
      "writeBufferDirtyPages",
      "write_buffer_dirty_pages"
    ),
    dbSizePages: numberField3("dbSizePages", "db_size_pages")
  };
}
async function queryOne3(database, sql, ...args) {
  const rows = await database.execute(sql, ...args);
  if (!rows[0]) throw new Error(`query returned no rows: ${sql}`);
  return rows[0];
}
async function storageStats2(database) {
  const [pageCount, freelistCount, pageSize] = await Promise.all([
    queryOne3(database, "PRAGMA page_count"),
    queryOne3(database, "PRAGMA freelist_count"),
    queryOne3(database, "PRAGMA page_size")
  ]);
  const nativeMetrics = await database.nativeMetrics?.();
  const copiedMetrics = copyNativeMetrics(nativeMetrics);
  return {
    page_count: pageCount.page_count,
    freelist_count: freelistCount.freelist_count,
    page_size: pageSize.page_size,
    vfs: copiedMetrics
  };
}
var sqliteMemoryPressure = actor47({
  options: {
    actionTimeout: 3e5
  },
  state: {
    sleepCount: 0
  },
  db: db12({
    onMigrate: async (database) => {
      await database.execute(`
				CREATE TABLE IF NOT EXISTS pressure_rows (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					seed TEXT NOT NULL,
					cycle INTEGER NOT NULL,
					bucket INTEGER NOT NULL,
					payload BLOB NOT NULL,
					touched_count INTEGER NOT NULL DEFAULT 0,
					created_at INTEGER NOT NULL
				)
			`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_pressure_rows_seed_cycle ON pressure_rows(seed, cycle)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_pressure_rows_bucket ON pressure_rows(bucket)"
      );
      await database.execute(`
				CREATE TABLE IF NOT EXISTS pressure_cycles (
					cycle INTEGER PRIMARY KEY,
					seed TEXT NOT NULL,
					inserted_rows INTEGER NOT NULL,
					deleted_rows INTEGER NOT NULL,
					active_rows INTEGER NOT NULL,
					active_bytes INTEGER NOT NULL,
					duration_ms REAL NOT NULL,
					created_at INTEGER NOT NULL
				)
			`);
    }
  }),
  onSleep: (c) => {
    c.state.sleepCount += 1;
    console.log(
      JSON.stringify({
        kind: "sqlite_memory_pressure_on_sleep",
        actorId: c.actorId,
        sleepCount: c.state.sleepCount,
        timestamp: (/* @__PURE__ */ new Date()).toISOString()
      })
    );
  },
  actions: {
    reset: async (c) => {
      await c.db.execute("DELETE FROM pressure_cycles");
      await c.db.execute("DELETE FROM pressure_rows");
      await c.db.execute("VACUUM");
      return {
        ok: true,
        storage: await storageStats2(c.db)
      };
    },
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    },
    releaseStorage: async (c) => {
      const before = await storageStats2(c.db);
      return {
        ok: true,
        before,
        after: await storageStats2(c.db)
      };
    },
    stats: async (c) => {
      const rowStats = await queryOne3(
        c.db,
        "SELECT COUNT(*) AS active_rows, COALESCE(SUM(length(payload)), 0) AS active_bytes, COALESCE(SUM(touched_count), 0) AS touched_sum FROM pressure_rows"
      );
      const cycles = await queryOne3(
        c.db,
        "SELECT COUNT(*) AS count FROM pressure_cycles"
      );
      const integrity = await queryOne3(
        c.db,
        "PRAGMA integrity_check"
      );
      return {
        activeRows: rowStats.active_rows,
        activeBytes: rowStats.active_bytes ?? 0,
        touchedCount: rowStats.touched_sum ?? 0,
        cycles: cycles.count,
        integrityCheck: integrity.integrity_check,
        storage: await storageStats2(c.db)
      };
    },
    runCycle: async (c, input) => {
      const startedAt = performance.now();
      const insertRows = finiteInt2(input.insertRows, DEFAULT_INSERT_ROWS);
      const rowBytes = finiteInt2(input.rowBytes, DEFAULT_ROW_BYTES3);
      const scanRows = Math.max(
        1,
        finiteInt2(input.scanRows, DEFAULT_SCAN_ROWS)
      );
      const now = Date.now();
      let insertedRows = 0;
      const logStage = (stage, phase, fields = {}) => {
        console.log(
          JSON.stringify({
            kind: "sqlite_memory_pressure_run_cycle_stage",
            actorId: c.actorId,
            seed: input.seed,
            cycle: input.cycle,
            stage,
            phase,
            elapsedMs: performance.now() - startedAt,
            timestamp: (/* @__PURE__ */ new Date()).toISOString(),
            ...fields
          })
        );
      };
      const executeTimed = async (stage, sql, ...args) => {
        const stageStartedAt = performance.now();
        logStage(stage, "start", { argCount: args.length });
        try {
          const rows = await c.db.execute(sql, ...args);
          logStage(stage, "end", {
            durationMs: performance.now() - stageStartedAt,
            rowCount: rows.length
          });
          return rows;
        } catch (err) {
          logStage(stage, "error", {
            durationMs: performance.now() - stageStartedAt,
            error: err instanceof Error ? err.message : String(err)
          });
          throw err;
        }
      };
      logStage("run_cycle", "start", {
        insertRows,
        rowBytes,
        scanRows
      });
      await executeTimed("begin", "BEGIN");
      try {
        while (insertedRows < insertRows) {
          const batchRows = Math.min(
            INSERT_BATCH_ROWS,
            insertRows - insertedRows
          );
          const placeholders = [];
          const args = [];
          for (let i = 0; i < batchRows; i += 1) {
            const rowIndex = insertedRows + i;
            placeholders.push("(?, ?, ?, randomblob(?), 0, ?)");
            args.push(
              input.seed,
              input.cycle,
              (input.cycle + rowIndex) % 32,
              rowBytes,
              now + rowIndex
            );
          }
          await executeTimed(
            "insert_batch",
            `INSERT INTO pressure_rows (seed, cycle, bucket, payload, touched_count, created_at) VALUES ${placeholders.join(", ")}`,
            ...args
          );
          insertedRows += batchRows;
          logStage("insert_batch_progress", "end", {
            insertedRows,
            batchRows
          });
        }
        await executeTimed("commit", "COMMIT");
      } catch (err) {
        await executeTimed("rollback", "ROLLBACK").catch(
          () => void 0
        );
        throw err;
      }
      const scan = await executeTimed(
        "scan_recent",
        "SELECT id, length(payload) AS payload_bytes FROM pressure_rows ORDER BY id DESC LIMIT ?",
        scanRows
      );
      const bucketAgg = await executeTimed(
        "bucket_agg",
        "SELECT bucket, COUNT(*) AS rows, SUM(length(payload)) AS bytes FROM pressure_rows WHERE bucket BETWEEN ? AND ? GROUP BY bucket ORDER BY bucket",
        input.cycle % 16,
        input.cycle % 16 + 15
      );
      await executeTimed(
        "touch_recent",
        "UPDATE pressure_rows SET touched_count = touched_count + 1 WHERE id IN (SELECT id FROM pressure_rows ORDER BY id DESC LIMIT ?)",
        Math.min(scanRows, insertRows)
      );
      const deletedRows = 0;
      const rowStatsRows = await executeTimed(
        "row_stats",
        "SELECT COUNT(*) AS active_rows, COALESCE(SUM(length(payload)), 0) AS active_bytes FROM pressure_rows"
      );
      const rowStats = rowStatsRows[0];
      if (!rowStats) throw new Error("query returned no rows: row_stats");
      const integrityRows = await executeTimed(
        "integrity_check",
        "PRAGMA integrity_check"
      );
      const integrity = integrityRows[0];
      if (!integrity) {
        throw new Error("query returned no rows: integrity_check");
      }
      const durationMs = performance.now() - startedAt;
      await executeTimed(
        "record_cycle",
        "INSERT OR REPLACE INTO pressure_cycles (cycle, seed, inserted_rows, deleted_rows, active_rows, active_bytes, duration_ms, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        input.cycle,
        input.seed,
        insertedRows,
        deletedRows,
        rowStats.active_rows,
        rowStats.active_bytes ?? 0,
        durationMs,
        now
      );
      const storageStartedAt = performance.now();
      logStage("storage_stats", "start");
      const storage = await storageStats2(c.db);
      logStage("storage_stats", "end", {
        durationMs: performance.now() - storageStartedAt,
        pageCount: storage.page_count,
        dbSizePages: storage.vfs?.dbSizePages ?? null,
        pageCacheEntries: storage.vfs?.pageCacheEntries ?? null
      });
      logStage("run_cycle", "end", {
        durationMs,
        activeRows: rowStats.active_rows,
        activeBytes: rowStats.active_bytes ?? 0,
        pageCount: storage.page_count
      });
      return {
        seed: input.seed,
        cycle: input.cycle,
        insertedRows,
        deletedRows,
        activeRows: rowStats.active_rows,
        activeBytes: rowStats.active_bytes ?? 0,
        scannedRows: scan.length,
        bucketsRead: bucketAgg.length,
        integrityCheck: integrity.integrity_check,
        storage,
        durationMs
      };
    }
  }
});

// src/actors/testing/sqlite-realworld-bench.ts
import { actor as actor48 } from "rivetkit";
import { db as db13 } from "rivetkit/db";
var DEFAULT_ROW_BYTES4 = 2 * 1024;
var ORDER_BATCH_ROWS = 50;
var DOC_BATCH_ROWS = 75;
var LEDGER_BATCH_ROWS = 100;
var POINT_LOOKUP_OPS = 1e3;
var RANGE_CHUNK_ROWS = 512;
var SETUP_TRANSACTION_ROWS = 128;
var FEED_PAGE_ROWS = 100;
var CHAT_LOG_CHUNK_BYTES = 4 * 1024;
var CHAT_LOG_INSERT_BATCH_SIZE = 50;
var CHAT_THREAD_ID = "rw-chat-main";
var SQL_RUSH_MSGS_COUNT = 2500;
var SQL_RUSH_TOOL_REFS_COUNT = 240;
var SQL_RUSH_EVENTS_COUNT = 700;
var SQL_RUSH_KV_COUNT = 40;
var SQL_RUSH_TOOLS_COUNT = 41;
var SQL_RUSH_META_COUNT = 12;
var WORKLOADS = [
  "small-rowid-point",
  "small-schema-read",
  "small-range-scan",
  "rowid-range-forward",
  "rowid-range-backward",
  "secondary-index-covering-range",
  "secondary-index-scattered-table",
  "aggregate-status",
  "aggregate-time-bucket",
  "aggregate-tenant-time-range",
  "parallel-read-aggregates",
  "parallel-read-write-transition",
  "feed-order-by-limit",
  "feed-pagination-adjacent",
  "join-order-items",
  "random-point-lookups",
  "hot-index-cold-table",
  "ledger-without-rowid-range",
  "chat-log-select-limit",
  "chat-log-select-indexed",
  "chat-log-count",
  "chat-log-sum",
  "chat-tool-read-fanout",
  "chat-tool-script",
  "write-batch-after-wake",
  "update-hot-partition",
  "delete-churn-range-read",
  "migration-create-indexes-large",
  "migration-create-indexes-skewed-large",
  "migration-table-rebuild-large",
  "migration-add-column-large",
  "migration-ddl-small"
];
function positiveInteger3(value, fallback, name) {
  const resolved = value ?? fallback;
  if (!Number.isInteger(resolved) || resolved < 1) {
    throw new Error(`${name} must be a positive integer`);
  }
  return resolved;
}
function assertWorkload(workload) {
  if (!WORKLOADS.includes(workload)) {
    throw new Error(`unknown SQLite benchmark workload: ${workload}`);
  }
}
function pseudoRandom(value) {
  return Math.imul(value ^ 2654435769, 2246822507) >>> 0;
}
function paddedHex(value) {
  return pseudoRandom(value).toString(16).padStart(8, "0");
}
function payload(prefix, bytes) {
  return prefix + "x".repeat(Math.max(0, bytes - prefix.length));
}
function typedRows2(rows) {
  return rows;
}
async function queryPageCount(database) {
  const [row] = typedRows2(
    await database.execute("PRAGMA page_count")
  );
  return row?.page_count ?? 0;
}
async function resetCommerce(database) {
  await database.execute("DELETE FROM rw_order_items");
  await database.execute("DELETE FROM rw_orders");
  await database.execute("DELETE FROM rw_customers");
  await database.execute("DELETE FROM rw_events");
}
async function resetDocs(database) {
  await database.execute("DELETE FROM rw_docs");
}
async function resetLedger(database) {
  await database.execute("DELETE FROM rw_ledger");
}
async function resetChatLog(database) {
  await database.execute("DELETE FROM rw_chat_log");
}
async function resetSqlRush(database) {
  await database.execute("DELETE FROM tool_refs");
  await database.execute("DELETE FROM msgs");
  await database.execute("DELETE FROM events");
  await database.execute("DELETE FROM kv");
  await database.execute("DELETE FROM tools");
  await database.execute("DELETE FROM meta");
}
async function resetMigration(database) {
  await database.execute(
    "DROP INDEX IF EXISTS idx_rw_migration_source_account"
  );
  await database.execute(
    "DROP INDEX IF EXISTS idx_rw_migration_source_created"
  );
  await database.execute(
    "DROP INDEX IF EXISTS idx_rw_migration_source_status_total"
  );
  await database.execute(
    "DROP INDEX IF EXISTS idx_rw_migration_source_skew_account"
  );
  await database.execute(
    "DROP INDEX IF EXISTS idx_rw_migration_source_skew_status"
  );
  await database.execute("DROP TABLE IF EXISTS rw_migration_source_rebuilt");
  await database.execute("DROP TABLE IF EXISTS rw_migration_source");
  await database.execute("DROP TABLE IF EXISTS rw_migration_audit");
  await database.execute("DROP TABLE IF EXISTS rw_migration_empty");
}
async function withTransaction(database, fn) {
  let inTransaction = false;
  await database.execute("BEGIN");
  inTransaction = true;
  try {
    await fn();
    await database.execute("COMMIT");
    inTransaction = false;
  } catch (err) {
    if (inTransaction) {
      await database.execute("ROLLBACK").catch(() => void 0);
    }
    throw err;
  }
}
async function seedCommerce(database, targetBytes, rowBytes) {
  await resetCommerce(database);
  const rows = Math.max(1, Math.ceil(targetBytes / rowBytes));
  const customerCount = Math.max(32, Math.ceil(rows / 16));
  const startedAt = performance.now();
  await withTransaction(database, async () => {
    for (let offset = 0; offset < customerCount; offset += ORDER_BATCH_ROWS) {
      const placeholders = [];
      const args = [];
      const batchEnd = Math.min(customerCount, offset + ORDER_BATCH_ROWS);
      for (let i = offset; i < batchEnd; i += 1) {
        placeholders.push("(?, ?, ?, ?, ?)");
        args.push(
          i + 1,
          `acct-${i % 64}`,
          `user-${paddedHex(i)}@example.test`,
          ["free", "pro", "team", "enterprise"][i % 4],
          ["iad", "sfo", "fra", "sin"][i % 4]
        );
      }
      await database.execute(
        `INSERT INTO rw_customers (id, account_id, email, plan, region) VALUES ${placeholders.join(", ")}`,
        ...args
      );
    }
  });
  for (let txStart = 0; txStart < rows; txStart += SETUP_TRANSACTION_ROWS) {
    const txEnd = Math.min(rows, txStart + SETUP_TRANSACTION_ROWS);
    await withTransaction(database, async () => {
      for (let offset = txStart; offset < txEnd; offset += ORDER_BATCH_ROWS) {
        const orderPlaceholders = [];
        const orderArgs = [];
        const itemPlaceholders = [];
        const itemArgs = [];
        const eventPlaceholders = [];
        const eventArgs = [];
        const batchEnd = Math.min(txEnd, offset + ORDER_BATCH_ROWS);
        for (let i = offset; i < batchEnd; i += 1) {
          const id = i + 1;
          const customerId = pseudoRandom(i) % customerCount + 1;
          const createdAt = 17e11 + i * 1e3;
          const status = ["pending", "paid", "shipped", "refunded"][i % 4];
          const totalCents = 500 + pseudoRandom(i + 17) % 25e3;
          const note = payload(`order-${id}-${status}:`, rowBytes);
          orderPlaceholders.push("(?, ?, ?, ?, ?, ?, ?)");
          orderArgs.push(
            id,
            customerId,
            createdAt,
            status,
            totalCents,
            i % 128,
            note
          );
          for (let item = 0; item < 2; item += 1) {
            itemPlaceholders.push("(?, ?, ?, ?, ?)");
            itemArgs.push(
              id,
              `sku-${paddedHex(i + item).slice(0, 6)}`,
              1 + (i + item) % 5,
              100 + pseudoRandom(i + item + 31) % 5e3,
              item
            );
          }
          eventPlaceholders.push("(?, ?, ?, ?, ?)");
          eventArgs.push(
            `acct-${customerId % 64}`,
            ["click", "purchase", "refund", "shipment"][i % 4],
            createdAt,
            `order:${id}`,
            payload(`event-${id}:`, Math.min(rowBytes, 512))
          );
        }
        await database.execute(
          `INSERT INTO rw_orders (id, customer_id, created_at, status, total_cents, shard, note) VALUES ${orderPlaceholders.join(", ")}`,
          ...orderArgs
        );
        await database.execute(
          `INSERT INTO rw_order_items (order_id, sku, quantity, price_cents, line_no) VALUES ${itemPlaceholders.join(", ")}`,
          ...itemArgs
        );
        await database.execute(
          `INSERT INTO rw_events (account_id, event_type, created_at, entity_key, properties) VALUES ${eventPlaceholders.join(", ")}`,
          ...eventArgs
        );
      }
    });
  }
  return {
    rows,
    targetBytes,
    rowBytes,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
async function seedDocs(database, targetBytes, rowBytes) {
  await resetDocs(database);
  const rows = Math.max(1, Math.ceil(targetBytes / rowBytes));
  const startedAt = performance.now();
  for (let txStart = 0; txStart < rows; txStart += SETUP_TRANSACTION_ROWS) {
    const txEnd = Math.min(rows, txStart + SETUP_TRANSACTION_ROWS);
    await withTransaction(database, async () => {
      for (let offset = txStart; offset < txEnd; offset += DOC_BATCH_ROWS) {
        const placeholders = [];
        const args = [];
        const batchEnd = Math.min(txEnd, offset + DOC_BATCH_ROWS);
        for (let i = offset; i < batchEnd; i += 1) {
          const rank = pseudoRandom(i);
          const body = payload(`doc-${i}-${rank}:`, rowBytes);
          placeholders.push("(?, ?, ?, ?, ?)");
          args.push(
            `doc-${paddedHex(i)}`,
            rank,
            `tenant-${rank % 128}`,
            body,
            rowBytes
          );
        }
        await database.execute(
          `INSERT INTO rw_docs (external_key, row_rank, tenant_id, body, body_bytes) VALUES ${placeholders.join(", ")}`,
          ...args
        );
      }
    });
  }
  return {
    rows,
    targetBytes,
    rowBytes,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
async function seedLedger(database, targetBytes, rowBytes) {
  await resetLedger(database);
  const rows = Math.max(1, Math.ceil(targetBytes / rowBytes));
  const startedAt = performance.now();
  for (let txStart = 0; txStart < rows; txStart += SETUP_TRANSACTION_ROWS) {
    const txEnd = Math.min(rows, txStart + SETUP_TRANSACTION_ROWS);
    await withTransaction(database, async () => {
      for (let offset = txStart; offset < txEnd; offset += LEDGER_BATCH_ROWS) {
        const placeholders = [];
        const args = [];
        const batchEnd = Math.min(txEnd, offset + LEDGER_BATCH_ROWS);
        for (let i = offset; i < batchEnd; i += 1) {
          const accountId = `acct-${String(i % 256).padStart(4, "0")}`;
          const entryId = Math.floor(i / 256) + 1;
          placeholders.push("(?, ?, ?, ?, ?)");
          args.push(
            accountId,
            entryId,
            (i % 2 === 0 ? 1 : -1) * (100 + i % 1e4),
            17e11 + i * 1e3,
            payload(
              `ledger-${accountId}-${entryId}:`,
              Math.min(rowBytes, 512)
            )
          );
        }
        await database.execute(
          `INSERT INTO rw_ledger (account_id, entry_id, amount_cents, created_at, memo) VALUES ${placeholders.join(", ")}`,
          ...args
        );
      }
    });
  }
  return {
    rows,
    targetBytes,
    rowBytes,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
function buildChatLogMessage(seq, targetBytes) {
  const prefix = `message-${seq}: `;
  return prefix + "x".repeat(Math.max(0, targetBytes - prefix.length));
}
async function seedChatLog(database, targetBytes) {
  await resetChatLog(database);
  const createdAtBase = 17e11;
  let remainingBytes = targetBytes;
  let rows = 0;
  const startedAt = performance.now();
  await withTransaction(database, async () => {
    while (remainingBytes > 0) {
      const placeholders = [];
      const args = [];
      for (let batchIndex = 0; batchIndex < CHAT_LOG_INSERT_BATCH_SIZE && remainingBytes > 0; batchIndex += 1) {
        const contentBytes = Math.min(
          CHAT_LOG_CHUNK_BYTES,
          remainingBytes
        );
        const seq = rows;
        const role = seq % 2 === 0 ? "user" : "assistant";
        placeholders.push("(?, ?, ?, ?, ?, ?, ?)");
        args.push(
          CHAT_THREAD_ID,
          seq,
          role,
          buildChatLogMessage(seq, contentBytes),
          contentBytes,
          Math.ceil(contentBytes / 4),
          createdAtBase + seq
        );
        remainingBytes -= contentBytes;
        rows += 1;
      }
      await database.execute(
        `INSERT INTO rw_chat_log (thread_id, seq, role, content, content_bytes, token_estimate, created_at) VALUES ${placeholders.join(", ")}`,
        ...args
      );
    }
  });
  return {
    rows,
    targetBytes,
    rowBytes: CHAT_LOG_CHUNK_BYTES,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
async function batchInsert3(database, sql, rows, batchSize) {
  if (rows.length === 0) return;
  const colsPerRow = rows[0]?.length ?? 0;
  if (colsPerRow === 0) return;
  const placeholder = `(${"?,".repeat(colsPerRow).slice(0, -1)})`;
  for (let i = 0; i < rows.length; i += batchSize) {
    const chunk = rows.slice(i, i + batchSize);
    const values = new Array(chunk.length).fill(placeholder).join(",");
    const args = [];
    for (const row of chunk) args.push(...row);
    await database.execute(`${sql} VALUES ${values}`, ...args);
  }
}
async function seedSqlRush(database, targetBytes) {
  await resetSqlRush(database);
  const now = 17e11;
  const startedAt = performance.now();
  await withTransaction(database, async () => {
    const msgsRows = [];
    for (let i = 0; i < SQL_RUSH_MSGS_COUNT; i += 1) {
      msgsRows.push([
        i === 0 ? null : i,
        i % 3 === 0 ? "user" : "assistant",
        payload("msg:", 512),
        0,
        now - (SQL_RUSH_MSGS_COUNT - i) * 1e3
      ]);
    }
    await batchInsert3(
      database,
      "INSERT INTO msgs (parent, role, content, cancelled, created_at)",
      msgsRows,
      50
    );
    const toolRefsRows = [];
    for (let i = 0; i < SQL_RUSH_TOOL_REFS_COUNT; i += 1) {
      toolRefsRows.push([
        i + 1,
        `tool_${i % 20}`,
        `call_${i}`,
        i % 5 === 0 ? "pending" : "done"
      ]);
    }
    await batchInsert3(
      database,
      "INSERT INTO tool_refs (msg_id, tool_name, tool_call_id, status)",
      toolRefsRows,
      100
    );
    const eventsRows = [];
    for (let i = 0; i < SQL_RUSH_EVENTS_COUNT; i += 1) {
      eventsRows.push([
        i + 1,
        `event_${i % 8}`,
        payload("event:", 256),
        now - (SQL_RUSH_EVENTS_COUNT - i) * 100
      ]);
    }
    await batchInsert3(
      database,
      "INSERT INTO events (seq, event_type, payload, created_at)",
      eventsRows,
      100
    );
    const kvRows = [];
    for (let i = 0; i < SQL_RUSH_KV_COUNT; i += 1) {
      kvRows.push([`kv_${i}`, payload("kv:", 128), now]);
    }
    await batchInsert3(
      database,
      "INSERT INTO kv (key, value, updated_at)",
      kvRows,
      40
    );
    const toolsRows = [];
    for (let i = 0; i < SQL_RUSH_TOOLS_COUNT; i += 1) {
      toolsRows.push([
        "exec-1",
        `tool_${i}`,
        payload("tool:", 1024),
        now
      ]);
    }
    await batchInsert3(
      database,
      "INSERT INTO tools (executor_id, name, spec, updated_at)",
      toolsRows,
      41
    );
    const metaRows = [];
    for (let i = 0; i < SQL_RUSH_META_COUNT; i += 1) {
      metaRows.push([`key_${i}`, payload("meta:", 64)]);
    }
    await batchInsert3(
      database,
      "INSERT INTO meta (key, value)",
      metaRows,
      12
    );
  });
  return {
    rows: SQL_RUSH_MSGS_COUNT + SQL_RUSH_TOOL_REFS_COUNT + SQL_RUSH_EVENTS_COUNT + SQL_RUSH_KV_COUNT + SQL_RUSH_TOOLS_COUNT + SQL_RUSH_META_COUNT,
    targetBytes,
    rowBytes: 0,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
async function seedMigrationSource(database, targetBytes, rowBytes, skewed = false) {
  await resetMigration(database);
  await database.execute(`CREATE TABLE rw_migration_source (
		id INTEGER PRIMARY KEY,
		account_id TEXT NOT NULL,
		status TEXT NOT NULL,
		created_at INTEGER NOT NULL,
		total_cents INTEGER NOT NULL,
		body TEXT NOT NULL
	)`);
  const rows = Math.max(1, Math.ceil(targetBytes / rowBytes));
  const startedAt = performance.now();
  for (let txStart = 0; txStart < rows; txStart += SETUP_TRANSACTION_ROWS) {
    const txEnd = Math.min(rows, txStart + SETUP_TRANSACTION_ROWS);
    await withTransaction(database, async () => {
      for (let offset = txStart; offset < txEnd; offset += ORDER_BATCH_ROWS) {
        const placeholders = [];
        const args = [];
        const batchEnd = Math.min(txEnd, offset + ORDER_BATCH_ROWS);
        for (let i = offset; i < batchEnd; i += 1) {
          const accountId = skewed ? `acct-${i % 10 === 0 ? i % 512 : i % 8}` : `acct-${pseudoRandom(i) % 512}`;
          const status = skewed ? i % 20 === 0 ? "failed" : "open" : ["open", "closed", "failed", "pending"][i % 4];
          placeholders.push("(?, ?, ?, ?, ?, ?)");
          args.push(
            i + 1,
            accountId,
            status,
            17e11 + i * 1e3,
            100 + pseudoRandom(i + 41) % 5e4,
            payload(`migration-${i}:`, rowBytes)
          );
        }
        await database.execute(
          `INSERT INTO rw_migration_source (id, account_id, status, created_at, total_cents, body) VALUES ${placeholders.join(", ")}`,
          ...args
        );
      }
    });
  }
  return {
    rows,
    targetBytes,
    rowBytes,
    setupMs: performance.now() - startedAt,
    pageCount: await queryPageCount(database)
  };
}
async function readRowidRange(database, direction) {
  const [count] = typedRows2(
    await database.execute("SELECT COUNT(*) AS rows FROM rw_orders")
  );
  const rows = count?.rows ?? 0;
  let bytes = 0;
  let scannedRows = 0;
  if (direction === "backward") {
    for (let upper = rows; upper > 0; upper -= RANGE_CHUNK_ROWS) {
      const lower = Math.max(1, upper - RANGE_CHUNK_ROWS + 1);
      const chunk = typedRows2(
        await database.execute(
          `SELECT length(note) AS bytes FROM rw_orders WHERE id BETWEEN ? AND ? ORDER BY id DESC`,
          lower,
          upper
        )
      );
      for (const row of chunk) {
        bytes += row.bytes;
        scannedRows += 1;
      }
    }
    return { rows: scannedRows, bytes };
  }
  for (let lower = 1; lower <= rows; lower += RANGE_CHUNK_ROWS) {
    const upper = lower + RANGE_CHUNK_ROWS - 1;
    const [chunk] = typedRows2(
      await database.execute(
        `SELECT COUNT(*) AS rows, COALESCE(SUM(length(note)), 0) AS bytes FROM rw_orders WHERE id BETWEEN ? AND ?`,
        lower,
        upper
      )
    );
    bytes += chunk?.bytes ?? 0;
    scannedRows += chunk?.rows ?? 0;
  }
  return { rows: scannedRows, bytes };
}
var sqliteRealworldBench = actor48({
  options: {
    actionTimeout: 12e5,
    sleepGracePeriod: 3e4
  },
  db: db13({
    onMigrate: async (database) => {
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_customers (
				id INTEGER PRIMARY KEY,
				account_id TEXT NOT NULL,
				email TEXT NOT NULL,
				plan TEXT NOT NULL,
				region TEXT NOT NULL
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_orders (
				id INTEGER PRIMARY KEY,
				customer_id INTEGER NOT NULL,
				created_at INTEGER NOT NULL,
				status TEXT NOT NULL,
				total_cents INTEGER NOT NULL,
				shard INTEGER NOT NULL,
				note TEXT NOT NULL
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_order_items (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				order_id INTEGER NOT NULL,
				sku TEXT NOT NULL,
				quantity INTEGER NOT NULL,
				price_cents INTEGER NOT NULL,
				line_no INTEGER NOT NULL
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_events (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				account_id TEXT NOT NULL,
				event_type TEXT NOT NULL,
				created_at INTEGER NOT NULL,
				entity_key TEXT NOT NULL,
				properties TEXT NOT NULL
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_docs (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				external_key TEXT NOT NULL UNIQUE,
				row_rank INTEGER NOT NULL,
				tenant_id TEXT NOT NULL,
				body TEXT NOT NULL,
				body_bytes INTEGER NOT NULL
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_ledger (
				account_id TEXT NOT NULL,
				entry_id INTEGER NOT NULL,
				amount_cents INTEGER NOT NULL,
				created_at INTEGER NOT NULL,
				memo TEXT NOT NULL,
				PRIMARY KEY (account_id, entry_id)
			) WITHOUT ROWID`);
      await database.execute(`CREATE TABLE IF NOT EXISTS rw_chat_log (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				thread_id TEXT NOT NULL,
				seq INTEGER NOT NULL,
				role TEXT NOT NULL,
				content TEXT NOT NULL,
				content_bytes INTEGER NOT NULL,
				token_estimate INTEGER NOT NULL,
				created_at INTEGER NOT NULL DEFAULT 0
			)`);
      await database.execute(
        "CREATE TABLE IF NOT EXISTS msgs (id INTEGER PRIMARY KEY AUTOINCREMENT, parent INTEGER, role TEXT NOT NULL, content TEXT NOT NULL, cancelled INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL)"
      );
      await database.execute(
        "CREATE TABLE IF NOT EXISTS tool_refs (id INTEGER PRIMARY KEY AUTOINCREMENT, msg_id INTEGER NOT NULL, tool_name TEXT NOT NULL, tool_call_id TEXT NOT NULL, status TEXT NOT NULL)"
      );
      await database.execute(
        "CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY, event_type TEXT NOT NULL, payload TEXT NOT NULL, created_at INTEGER NOT NULL)"
      );
      await database.execute(
        "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL)"
      );
      await database.execute(
        "CREATE TABLE IF NOT EXISTS tools (id INTEGER PRIMARY KEY AUTOINCREMENT, executor_id TEXT NOT NULL, name TEXT NOT NULL, spec TEXT NOT NULL, updated_at INTEGER NOT NULL)"
      );
      await database.execute(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_orders_customer_created ON rw_orders(customer_id, created_at DESC)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_orders_status_created ON rw_orders(status, created_at)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_orders_created ON rw_orders(created_at DESC)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_order_items_order ON rw_order_items(order_id)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_events_account_created ON rw_events(account_id, created_at)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_docs_external_rank ON rw_docs(external_key, row_rank)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_docs_tenant_rank ON rw_docs(tenant_id, row_rank)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_rw_chat_log_thread_seq ON rw_chat_log(thread_id, seq DESC)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_msgs_parent_role_cancelled_created_at ON msgs (parent, role, cancelled, created_at)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_tool_refs_msg_id ON tool_refs (msg_id)"
      );
    }
  }),
  actions: {
    inspectCacheConfig: async (c) => {
      const [cacheSize] = typedRows2(
        await c.db.execute("PRAGMA cache_size")
      );
      const [pageSize] = typedRows2(
        await c.db.execute("PRAGMA page_size")
      );
      return {
        sqliteCacheSizePragma: cacheSize?.cache_size ?? null,
        sqlitePageSize: pageSize?.page_size ?? null,
        pageCount: await queryPageCount(c.db)
      };
    },
    setupWorkload: async (c, input) => {
      assertWorkload(input.workload);
      const rowBytes = positiveInteger3(
        input.rowBytes,
        DEFAULT_ROW_BYTES4,
        "rowBytes"
      );
      if (input.workload === "migration-ddl-small") {
        await resetMigration(c.db);
        return {
          rows: 0,
          targetBytes: 0,
          rowBytes,
          setupMs: 0,
          pageCount: await queryPageCount(c.db)
        };
      }
      const targetBytes = positiveInteger3(
        input.targetBytes,
        8 * 1024 * 1024,
        "targetBytes"
      );
      switch (input.workload) {
        case "small-rowid-point":
        case "small-schema-read":
        case "small-range-scan":
        case "rowid-range-forward":
        case "rowid-range-backward":
        case "aggregate-status":
        case "aggregate-time-bucket":
        case "aggregate-tenant-time-range":
        case "parallel-read-aggregates":
        case "parallel-read-write-transition":
        case "feed-order-by-limit":
        case "feed-pagination-adjacent":
        case "join-order-items":
        case "random-point-lookups":
        case "write-batch-after-wake":
        case "update-hot-partition":
        case "delete-churn-range-read":
          return seedCommerce(c.db, targetBytes, rowBytes);
        case "secondary-index-covering-range":
        case "secondary-index-scattered-table":
        case "hot-index-cold-table":
          return seedDocs(c.db, targetBytes, rowBytes);
        case "ledger-without-rowid-range":
          return seedLedger(c.db, targetBytes, rowBytes);
        case "chat-log-select-limit":
        case "chat-log-select-indexed":
        case "chat-log-count":
        case "chat-log-sum":
        case "chat-tool-read-fanout":
          return seedChatLog(c.db, targetBytes);
        case "chat-tool-script":
          return seedSqlRush(c.db, targetBytes);
        case "migration-create-indexes-large":
          return seedMigrationSource(c.db, targetBytes, rowBytes);
        case "migration-create-indexes-skewed-large":
          return seedMigrationSource(
            c.db,
            targetBytes,
            rowBytes,
            true
          );
        case "migration-table-rebuild-large":
        case "migration-add-column-large":
          return seedMigrationSource(c.db, targetBytes, rowBytes);
      }
    },
    runWorkload: async (c, input) => {
      assertWorkload(input.workload);
      const t0 = performance.now();
      let details;
      switch (input.workload) {
        case "small-rowid-point": {
          let bytes = 0;
          for (let i = 0; i < 50; i += 1) {
            const id = i % 16 + 1;
            const [row] = typedRows2(
              await c.db.execute(
                "SELECT length(note) AS bytes FROM rw_orders WHERE id = ?",
                id
              )
            );
            bytes += row?.bytes ?? 0;
          }
          details = { ops: 50, bytes };
          break;
        }
        case "small-schema-read": {
          const tables = await c.db.execute(
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'index') ORDER BY name"
          );
          const columns = await c.db.execute(
            "PRAGMA table_info(rw_orders)"
          );
          const [count] = typedRows2(
            await c.db.execute(
              "SELECT COUNT(*) AS rows FROM rw_orders"
            )
          );
          details = {
            objects: tables.length,
            columns: columns.length,
            rows: count?.rows ?? 0
          };
          break;
        }
        case "small-range-scan":
        case "rowid-range-forward": {
          details = await readRowidRange(c.db, "forward");
          break;
        }
        case "rowid-range-backward": {
          details = await readRowidRange(c.db, "backward");
          break;
        }
        case "secondary-index-covering-range": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT external_key, row_rank FROM rw_docs
						WHERE external_key BETWEEN 'doc-00000000' AND 'doc-ffffffff'
						ORDER BY external_key`
            )
          );
          let checksum2 = 0;
          for (const row of rows)
            checksum2 = checksum2 + row.row_rank >>> 0;
          details = { rows: rows.length, checksum: checksum2 };
          break;
        }
        case "secondary-index-scattered-table": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT body_bytes AS bytes FROM rw_docs
						WHERE external_key BETWEEN 'doc-00000000' AND 'doc-ffffffff'
						ORDER BY external_key`
            )
          );
          let bytes = 0;
          for (const row of rows) bytes += row.bytes;
          details = { rows: rows.length, bytes };
          break;
        }
        case "aggregate-status": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT status, COUNT(*) AS rows, SUM(total_cents) AS total
						FROM rw_orders
						GROUP BY status
						ORDER BY status`
            )
          );
          details = {
            groups: rows.length,
            rows: rows.reduce((sum, row) => sum + row.rows, 0),
            total: rows.reduce((sum, row) => sum + row.total, 0)
          };
          break;
        }
        case "aggregate-time-bucket": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT (created_at / 300000) AS bucket, COUNT(*) AS rows, SUM(total_cents) AS total
						FROM rw_orders
						GROUP BY bucket
						ORDER BY bucket`
            )
          );
          details = {
            buckets: rows.length,
            rows: rows.reduce((sum, row) => sum + row.rows, 0),
            total: rows.reduce((sum, row) => sum + row.total, 0)
          };
          break;
        }
        case "aggregate-tenant-time-range": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT e.event_type, COUNT(*) AS rows, SUM(o.total_cents) AS total
						FROM rw_events e
						JOIN rw_orders o ON o.id = CAST(substr(e.entity_key, 7) AS INTEGER)
						WHERE e.account_id = ? AND e.created_at BETWEEN ? AND ?
						GROUP BY e.event_type
						ORDER BY e.event_type`,
              "acct-7",
              17e11,
              17e11 + 864e5
            )
          );
          details = {
            groups: rows.length,
            rows: rows.reduce((sum, row) => sum + row.rows, 0),
            total: rows.reduce((sum, row) => sum + row.total, 0)
          };
          break;
        }
        case "parallel-read-aggregates": {
          const [statusRows, bucketRows, tenantRows, joinRows] = await Promise.all([
            c.db.execute(
              `SELECT status, COUNT(*) AS rows, SUM(total_cents) AS total
						FROM rw_orders
						GROUP BY status
						ORDER BY status`
            ),
            c.db.execute(
              `SELECT (created_at / 300000) AS bucket, COUNT(*) AS rows, SUM(total_cents) AS total
						FROM rw_orders
						GROUP BY bucket
						ORDER BY bucket`
            ),
            c.db.execute(
              `SELECT e.event_type, COUNT(*) AS rows, SUM(o.total_cents) AS total
						FROM rw_events e
						JOIN rw_orders o ON o.id = CAST(substr(e.entity_key, 7) AS INTEGER)
						WHERE e.account_id = ? AND e.created_at BETWEEN ? AND ?
						GROUP BY e.event_type
						ORDER BY e.event_type`,
              "acct-7",
              17e11,
              17e11 + 864e5
            ),
            c.db.execute(
              `SELECT o.status, COUNT(*) AS rows, SUM(oi.quantity * oi.price_cents) AS total
						FROM rw_orders o
						JOIN rw_order_items oi ON oi.order_id = o.id
						GROUP BY o.status
						ORDER BY o.status`
            )
          ]);
          const aggregates = [
            ...typedRows2(statusRows),
            ...typedRows2(bucketRows),
            ...typedRows2(tenantRows),
            ...typedRows2(joinRows)
          ];
          details = {
            ops: 4,
            groups: aggregates.length,
            rows: aggregates.reduce(
              (sum, row) => sum + row.rows,
              0
            ),
            total: aggregates.reduce(
              (sum, row) => sum + row.total,
              0
            )
          };
          break;
        }
        case "parallel-read-write-transition": {
          const readStatus = c.db.execute(
            `SELECT status, COUNT(*) AS rows, SUM(total_cents) AS total
						FROM rw_orders
						GROUP BY status
						ORDER BY status`
          );
          const readJoin = c.db.execute(
            `SELECT o.status, COUNT(*) AS rows, SUM(oi.quantity * oi.price_cents) AS total
						FROM rw_orders o
						JOIN rw_order_items oi ON oi.order_id = o.id
						GROUP BY o.status
						ORDER BY o.status`
          );
          const writeHotShard = c.db.execute(
            "UPDATE rw_orders SET total_cents = total_cents + 1 WHERE shard BETWEEN 0 AND 7"
          );
          const readAfterWrite = c.db.execute(
            "SELECT COUNT(*) AS rows FROM rw_orders WHERE shard BETWEEN 0 AND 7"
          );
          const [statusRows, joinRows, , shardRows] = await Promise.all([
            readStatus,
            readJoin,
            writeHotShard,
            readAfterWrite
          ]);
          const aggregates = [
            ...typedRows2(statusRows),
            ...typedRows2(joinRows)
          ];
          const [shardCount] = typedRows2(shardRows);
          details = {
            ops: 4,
            readOps: 3,
            writeOps: 1,
            groups: aggregates.length,
            rows: aggregates.reduce((sum, row) => sum + row.rows, 0) + (shardCount?.rows ?? 0),
            total: aggregates.reduce(
              (sum, row) => sum + row.total,
              0
            )
          };
          break;
        }
        case "feed-order-by-limit": {
          const rows = await c.db.execute(
            `SELECT id, customer_id, created_at, status, total_cents
						FROM rw_orders
						WHERE created_at >= ?
						ORDER BY created_at DESC
						LIMIT 1000`,
            17e11
          );
          details = { rows: rows.length };
          break;
        }
        case "feed-pagination-adjacent": {
          const firstPage = typedRows2(
            await c.db.execute(
              `SELECT created_at
						FROM rw_orders
						WHERE created_at >= ?
						ORDER BY created_at DESC
						LIMIT ?`,
              17e11,
              FEED_PAGE_ROWS
            )
          );
          const cursor = firstPage.at(-1)?.created_at ?? 17e11;
          const secondPage = await c.db.execute(
            `SELECT id, customer_id, created_at, status, total_cents
						FROM rw_orders
						WHERE created_at < ?
						ORDER BY created_at DESC
						LIMIT ?`,
            cursor,
            FEED_PAGE_ROWS
          );
          details = {
            firstPageRows: firstPage.length,
            rows: secondPage.length
          };
          break;
        }
        case "join-order-items": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT o.status, COUNT(*) AS rows, SUM(oi.quantity * oi.price_cents) AS total
						FROM rw_orders o
						JOIN rw_order_items oi ON oi.order_id = o.id
						GROUP BY o.status
						ORDER BY o.status`
            )
          );
          details = {
            groups: rows.length,
            rows: rows.reduce((sum, row) => sum + row.rows, 0),
            total: rows.reduce((sum, row) => sum + row.total, 0)
          };
          break;
        }
        case "random-point-lookups": {
          const [count] = typedRows2(
            await c.db.execute(
              "SELECT COUNT(*) AS rows FROM rw_orders"
            )
          );
          const rows = Math.max(1, count?.rows ?? 1);
          let bytes = 0;
          for (let i = 0; i < POINT_LOOKUP_OPS; i += 1) {
            const id = pseudoRandom(i) % rows + 1;
            const [row] = typedRows2(
              await c.db.execute(
                "SELECT length(note) AS bytes FROM rw_orders WHERE id = ?",
                id
              )
            );
            bytes += row?.bytes ?? 0;
          }
          details = { ops: POINT_LOOKUP_OPS, bytes };
          break;
        }
        case "hot-index-cold-table": {
          const indexRows = typedRows2(
            await c.db.execute(
              `SELECT id
						FROM rw_docs
						WHERE tenant_id = ?
						ORDER BY row_rank
						LIMIT 1000`,
              "tenant-7"
            )
          );
          let bytes = 0;
          for (const row of indexRows) {
            const [doc] = typedRows2(
              await c.db.execute(
                "SELECT body_bytes AS bytes FROM rw_docs WHERE id = ?",
                row.id
              )
            );
            bytes += doc?.bytes ?? 0;
          }
          details = { rows: indexRows.length, bytes };
          break;
        }
        case "ledger-without-rowid-range": {
          const rows = typedRows2(
            await c.db.execute(
              `SELECT account_id, entry_id, amount_cents, length(memo) AS bytes
						FROM rw_ledger
						WHERE account_id BETWEEN 'acct-0040' AND 'acct-0180'
						ORDER BY account_id, entry_id`
            )
          );
          let bytes = 0;
          for (const row of rows) bytes += row.bytes;
          details = { rows: rows.length, bytes };
          break;
        }
        case "chat-log-select-limit": {
          const rows = await c.db.execute(
            "SELECT seq, role, substr(content, 1, 128) AS preview FROM rw_chat_log ORDER BY created_at DESC LIMIT 100"
          );
          details = { rows: rows.length };
          break;
        }
        case "chat-log-select-indexed": {
          const expectedRows = Math.max(
            1,
            Math.ceil(
              positiveInteger3(
                input.targetBytes,
                CHAT_LOG_CHUNK_BYTES,
                "targetBytes"
              ) / CHAT_LOG_CHUNK_BYTES
            )
          );
          const lowerBound = Math.max(0, expectedRows - 100);
          const rows = await c.db.execute(
            "SELECT seq, role, content_bytes FROM rw_chat_log WHERE thread_id = ? AND seq >= ? ORDER BY seq DESC LIMIT 100",
            CHAT_THREAD_ID,
            lowerBound
          );
          details = { rows: rows.length };
          break;
        }
        case "chat-log-count": {
          const [row] = typedRows2(
            await c.db.execute(
              "SELECT COUNT(*) AS count FROM rw_chat_log WHERE thread_id = ?",
              CHAT_THREAD_ID
            )
          );
          details = { ops: 1, rows: row?.count ?? 0 };
          break;
        }
        case "chat-log-sum": {
          const [row] = typedRows2(
            await c.db.execute(
              "SELECT SUM(content_bytes) AS total_bytes FROM rw_chat_log WHERE thread_id = ?",
              CHAT_THREAD_ID
            )
          );
          details = { ops: 1, bytes: row?.total_bytes ?? 0 };
          break;
        }
        case "chat-tool-read-fanout": {
          const expectedRows = Math.max(
            1,
            Math.ceil(
              positiveInteger3(
                input.targetBytes,
                CHAT_LOG_CHUNK_BYTES,
                "targetBytes"
              ) / CHAT_LOG_CHUNK_BYTES
            )
          );
          const lowerBound = Math.max(0, expectedRows - 100);
          const [limitRows, indexedRows, countRows, sumRows] = await Promise.all([
            c.db.execute(
              "SELECT seq, role, substr(content, 1, 128) AS preview FROM rw_chat_log ORDER BY created_at DESC LIMIT 100"
            ),
            c.db.execute(
              "SELECT seq, role, content_bytes FROM rw_chat_log WHERE thread_id = ? AND seq >= ? ORDER BY seq DESC LIMIT 100",
              CHAT_THREAD_ID,
              lowerBound
            ),
            c.db.execute(
              "SELECT COUNT(*) AS count FROM rw_chat_log WHERE thread_id = ?",
              CHAT_THREAD_ID
            ),
            c.db.execute(
              "SELECT SUM(content_bytes) AS total_bytes FROM rw_chat_log WHERE thread_id = ?",
              CHAT_THREAD_ID
            )
          ]);
          const [countRow] = typedRows2(countRows);
          const [sumRow] = typedRows2(
            sumRows
          );
          details = {
            ops: 4,
            limitRows: limitRows.length,
            indexedRows: indexedRows.length,
            rows: countRow?.count ?? 0,
            bytes: sumRow?.total_bytes ?? 0
          };
          break;
        }
        case "chat-tool-script": {
          const [
            msgsRows,
            toolRefsRows,
            eventsRows,
            kvRows,
            toolsRows,
            metaRows,
            unresolvedRows
          ] = await Promise.all([
            c.db.execute(
              "SELECT id, role, length(content) AS bytes FROM msgs WHERE parent IS NOT NULL AND role = ? AND cancelled = 0 ORDER BY created_at DESC LIMIT 50",
              "assistant"
            ),
            c.db.execute(
              "SELECT id, tool_name, status FROM tool_refs WHERE status = ? ORDER BY id DESC LIMIT 50",
              "pending"
            ),
            c.db.execute(
              "SELECT seq, event_type, length(payload) AS bytes FROM events WHERE seq > ? ORDER BY seq ASC LIMIT 100",
              600
            ),
            c.db.execute(
              "SELECT key, length(value) AS bytes FROM kv ORDER BY updated_at DESC LIMIT 20"
            ),
            c.db.execute(
              "SELECT id, name, length(spec) AS bytes FROM tools WHERE executor_id = ? ORDER BY updated_at DESC",
              "exec-1"
            ),
            c.db.execute(
              "SELECT key, length(value) AS bytes FROM meta"
            ),
            c.db.execute(`SELECT m.id, m.role, count(tr.id) AS pending_refs
							FROM msgs m
							LEFT JOIN tool_refs tr ON tr.msg_id = m.id AND tr.status = 'pending'
							WHERE m.role = 'assistant' AND m.cancelled = 0
							GROUP BY m.id
							ORDER BY m.created_at DESC
							LIMIT 100`)
          ]);
          details = {
            ops: 7,
            msgsRows: msgsRows.length,
            toolRefsRows: toolRefsRows.length,
            eventsRows: eventsRows.length,
            kvRows: kvRows.length,
            toolsRows: toolsRows.length,
            metaRows: metaRows.length,
            unresolvedRows: unresolvedRows.length
          };
          break;
        }
        case "write-batch-after-wake": {
          const [count] = typedRows2(
            await c.db.execute(
              "SELECT COUNT(*) AS rows FROM rw_orders"
            )
          );
          const startId = (count?.rows ?? 0) + 1;
          await c.db.execute("BEGIN");
          for (let offset = 0; offset < 1e3; offset += ORDER_BATCH_ROWS) {
            const placeholders = [];
            const args = [];
            for (let i = offset; i < offset + ORDER_BATCH_ROWS; i += 1) {
              const id = startId + i;
              placeholders.push("(?, ?, ?, ?, ?, ?, ?)");
              args.push(
                id,
                i % 128 + 1,
                18e11 + i,
                "pending",
                1e3 + i,
                i % 128,
                payload(
                  `wake-insert-${id}:`,
                  DEFAULT_ROW_BYTES4
                )
              );
            }
            await c.db.execute(
              `INSERT INTO rw_orders (id, customer_id, created_at, status, total_cents, shard, note) VALUES ${placeholders.join(", ")}`,
              ...args
            );
          }
          await c.db.execute("COMMIT");
          details = { rows: 1e3 };
          break;
        }
        case "update-hot-partition": {
          await c.db.execute(
            "UPDATE rw_orders SET total_cents = total_cents + 1 WHERE shard BETWEEN 0 AND 15"
          );
          const [count] = typedRows2(
            await c.db.execute(
              "SELECT COUNT(*) AS rows FROM rw_orders WHERE shard BETWEEN 0 AND 15"
            )
          );
          details = { rows: count?.rows ?? 0 };
          break;
        }
        case "delete-churn-range-read": {
          await c.db.execute(
            "DELETE FROM rw_orders WHERE shard BETWEEN 0 AND 15"
          );
          const result = await readRowidRange(c.db, "forward");
          details = {
            ...result,
            deletedShardCount: 16
          };
          break;
        }
        case "migration-create-indexes-large": {
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_source_account ON rw_migration_source(account_id)"
          );
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_source_created ON rw_migration_source(created_at)"
          );
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_source_status_total ON rw_migration_source(status, total_cents)"
          );
          details = { indexes: 3 };
          break;
        }
        case "migration-create-indexes-skewed-large": {
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_source_skew_account ON rw_migration_source(account_id, created_at)"
          );
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_source_skew_status ON rw_migration_source(status, total_cents)"
          );
          details = { indexes: 2, skewed: true };
          break;
        }
        case "migration-table-rebuild-large": {
          await c.db.execute(`CREATE TABLE rw_migration_source_rebuilt (
						id INTEGER PRIMARY KEY,
						account_id TEXT NOT NULL,
						status TEXT NOT NULL,
						created_at INTEGER NOT NULL,
						total_cents INTEGER NOT NULL,
						body TEXT NOT NULL,
						archived_at INTEGER
					)`);
          await c.db.execute(`INSERT INTO rw_migration_source_rebuilt (
						id, account_id, status, created_at, total_cents, body, archived_at
					)
					SELECT id, account_id, status, created_at, total_cents, body, NULL
					FROM rw_migration_source`);
          await c.db.execute("DROP TABLE rw_migration_source");
          await c.db.execute(
            "ALTER TABLE rw_migration_source_rebuilt RENAME TO rw_migration_source"
          );
          details = { rebuilt: true };
          break;
        }
        case "migration-add-column-large": {
          await c.db.execute(
            "ALTER TABLE rw_migration_source ADD COLUMN archived_at INTEGER"
          );
          details = { alters: 1, rewritesRows: false };
          break;
        }
        case "migration-ddl-small": {
          await c.db.execute(`CREATE TABLE rw_migration_empty (
						id INTEGER PRIMARY KEY,
						tenant_id TEXT NOT NULL,
						created_at INTEGER NOT NULL
					)`);
          await c.db.execute(
            "ALTER TABLE rw_migration_empty ADD COLUMN status TEXT"
          );
          await c.db.execute(
            "CREATE INDEX idx_rw_migration_empty_tenant_created ON rw_migration_empty(tenant_id, created_at)"
          );
          await c.db.execute(`CREATE TABLE rw_migration_audit (
						id INTEGER PRIMARY KEY,
						migration_name TEXT NOT NULL,
						applied_at INTEGER NOT NULL
					)`);
          details = { tables: 2, indexes: 1, alters: 1 };
          break;
        }
      }
      const ms = performance.now() - t0;
      return {
        ms,
        workload: input.workload,
        ...details,
        pageCount: await queryPageCount(c.db)
      };
    },
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    }
  }
});

// src/actors/testing/test-counter.ts
import { actor as actor49 } from "rivetkit";
var testCounter = actor49({
  state: { count: 0 },
  actions: {
    increment: (c, amount = 1) => {
      c.state.count += amount;
      return c.state.count;
    },
    getCount: (c) => {
      return c.state.count;
    },
    reset: (c) => {
      c.state.count = 0;
      return c.state.count;
    }
  }
});

// src/actors/testing/test-counter-sqlite.ts
import { actor as actor50 } from "rivetkit";
import { db as db14 } from "rivetkit/db";
var testCounterSqlite = actor50({
  db: db14({
    onMigrate: async (db17) => {
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS counter (
					id INTEGER PRIMARY KEY CHECK (id = 1),
					value INTEGER NOT NULL DEFAULT 0
				)
			`);
      await db17.execute(
        "INSERT OR IGNORE INTO counter (id, value) VALUES (1, 0)"
      );
    }
  }),
  actions: {
    increment: async (c, amount = 1) => {
      await c.db.execute(
        "UPDATE counter SET value = value + ? WHERE id = 1",
        amount
      );
      const rows = await c.db.execute(
        "SELECT value FROM counter WHERE id = 1"
      );
      return rows[0].value;
    },
    getCount: async (c) => {
      const rows = await c.db.execute(
        "SELECT value FROM counter WHERE id = 1"
      );
      return rows[0].value;
    },
    reset: async (c) => {
      await c.db.execute("UPDATE counter SET value = 0 WHERE id = 1");
      return 0;
    }
  }
});

// src/actors/testing/test-sqlite-bench.ts
import { actor as actor51 } from "rivetkit";
import { db as db15 } from "rivetkit/db";
var CHAT_LOG_CHUNK_BYTES2 = 4 * 1024;
var CHAT_LOG_INSERT_BATCH_SIZE2 = 50;
function buildChatLogMessage2(seq, targetBytes) {
  const prefix = `message-${seq}: `;
  return prefix + "x".repeat(Math.max(0, targetBytes - prefix.length));
}
async function seedChatLog2(database, targetBytes) {
  const threadId = `chat-${crypto.randomUUID()}`;
  const createdAtBase = Date.now();
  let remainingBytes = targetBytes;
  let rows = 0;
  await database.execute("BEGIN");
  try {
    while (remainingBytes > 0) {
      const placeholders = [];
      const args = [];
      for (let batchIndex = 0; batchIndex < CHAT_LOG_INSERT_BATCH_SIZE2 && remainingBytes > 0; batchIndex++) {
        const contentBytes = Math.min(
          CHAT_LOG_CHUNK_BYTES2,
          remainingBytes
        );
        const seq = rows;
        const role = seq % 2 === 0 ? "user" : "assistant";
        placeholders.push("(?, ?, ?, ?, ?, ?, ?)");
        args.push(
          threadId,
          seq,
          role,
          buildChatLogMessage2(seq, contentBytes),
          contentBytes,
          Math.ceil(contentBytes / 4),
          createdAtBase + seq
        );
        remainingBytes -= contentBytes;
        rows++;
      }
      await database.execute(
        `INSERT INTO chat_log (thread_id, seq, role, content, content_bytes, token_estimate, created_at) VALUES ${placeholders.join(", ")}`,
        ...args
      );
    }
    await database.execute("COMMIT");
  } catch (err) {
    await database.execute("ROLLBACK");
    throw err;
  }
  return { threadId, rows, totalBytes: targetBytes };
}
var testSqliteBench = actor51({
  options: {
    actionTimeout: 3e5
  },
  db: db15({
    onMigrate: async (database) => {
      await database.execute(`CREATE TABLE IF NOT EXISTS bench (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				key TEXT NOT NULL,
				value TEXT NOT NULL,
				num INTEGER NOT NULL DEFAULT 0,
				payload BLOB,
				created_at INTEGER NOT NULL DEFAULT 0
			)`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_bench_key ON bench(key)"
      );
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_bench_num ON bench(num)"
      );
      await database.execute(`CREATE TABLE IF NOT EXISTS bench_json (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				data TEXT NOT NULL DEFAULT '{}'
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS bench_secondary (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				bench_id INTEGER NOT NULL,
				label TEXT NOT NULL,
				score REAL NOT NULL DEFAULT 0,
				FOREIGN KEY (bench_id) REFERENCES bench(id)
			)`);
      await database.execute(`CREATE TABLE IF NOT EXISTS chat_log (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				thread_id TEXT NOT NULL,
				seq INTEGER NOT NULL,
				role TEXT NOT NULL,
				content TEXT NOT NULL,
				content_bytes INTEGER NOT NULL,
				token_estimate INTEGER NOT NULL,
				created_at INTEGER NOT NULL DEFAULT 0
			)`);
      await database.execute(
        "CREATE INDEX IF NOT EXISTS idx_chat_log_thread_seq ON chat_log(thread_id, seq DESC)"
      );
    }
  }),
  actions: {
    noop: (_c) => ({ ok: true }),
    goToSleep: (c) => {
      c.sleep();
      return { ok: true };
    },
    insertSingle: async (c, n) => {
      const t0 = performance.now();
      for (let i = 0; i < n; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `k-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      return { ms: performance.now() - t0, ops: n };
    },
    insertTx: async (c, n) => {
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < n; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `k-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: n };
    },
    insertBatch: async (c, n) => {
      const t0 = performance.now();
      const placeholders = Array.from(
        { length: n },
        () => "(?, ?, ?, ?)"
      ).join(", ");
      const args = [];
      for (let i = 0; i < n; i++) {
        args.push(`k-${i}`, `v-${i}`, i, Date.now());
      }
      await c.db.execute(
        `INSERT INTO bench (key, value, num, created_at) VALUES ${placeholders}`,
        ...args
      );
      return { ms: performance.now() - t0, ops: n };
    },
    pointRead: async (c, n) => {
      await c.db.execute(
        "INSERT INTO bench (key, value, num, created_at) VALUES ('pr', 'pr', 0, 0)"
      );
      const rows = await c.db.execute(
        "SELECT id FROM bench WHERE key = 'pr' LIMIT 1"
      );
      const id = rows[0].id;
      const t0 = performance.now();
      for (let i = 0; i < n; i++) {
        await c.db.execute("SELECT * FROM bench WHERE id = ?", id);
      }
      return { ms: performance.now() - t0, ops: n };
    },
    fullScan: async (c, seedRows) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < seedRows; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `scan-${i}`,
          `val-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute("SELECT * FROM bench");
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    rangeScanIndexed: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `rs-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT * FROM bench WHERE num BETWEEN 100 AND 300"
      );
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    rangeScanUnindexed: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `ru-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT * FROM bench WHERE value BETWEEN 'v-100' AND 'v-300'"
      );
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    bulkUpdate: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `bu-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      await c.db.execute(
        "UPDATE bench SET value = 'updated', num = num + 1000 WHERE key LIKE 'bu-%'"
      );
      return { ms: performance.now() - t0, seedMs, ops: 200 };
    },
    bulkDelete: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `bd-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      await c.db.execute("DELETE FROM bench WHERE key LIKE 'bd-%'");
      return { ms: performance.now() - t0, seedMs, ops: 200 };
    },
    hotRowUpdates: async (c, n) => {
      await c.db.execute(
        "INSERT INTO bench (key, value, num, created_at) VALUES ('hot', 'v', 0, 0)"
      );
      const rows = await c.db.execute(
        "SELECT id FROM bench WHERE key = 'hot' LIMIT 1"
      );
      const id = rows[0].id;
      const t0 = performance.now();
      for (let i = 0; i < n; i++) {
        await c.db.execute(
          "UPDATE bench SET num = ? WHERE id = ?",
          i,
          id
        );
      }
      return { ms: performance.now() - t0, ops: n };
    },
    vacuumAfterDelete: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `vac-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      await c.db.execute("DELETE FROM bench WHERE key LIKE 'vac-%'");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      await c.db.execute("VACUUM");
      return { ms: performance.now() - t0, seedMs };
    },
    largePayloadInsert: async (c, n) => {
      const blob = "x".repeat(32 * 1024);
      const t0 = performance.now();
      for (let i = 0; i < n; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, payload, created_at) VALUES (?, ?, ?, ?, ?)",
          `lp-${i}`,
          `v-${i}`,
          i,
          blob,
          Date.now()
        );
      }
      return { ms: performance.now() - t0, ops: n };
    },
    mixedOltp: async (c) => {
      const t0 = performance.now();
      await c.db.execute(
        "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
        "oltp",
        "initial",
        0,
        Date.now()
      );
      const rows = await c.db.execute(
        "SELECT * FROM bench WHERE key = 'oltp' LIMIT 1"
      );
      const id = rows[0].id;
      await c.db.execute(
        "UPDATE bench SET value = 'updated', num = 1 WHERE id = ?",
        id
      );
      await c.db.execute("SELECT * FROM bench WHERE id = ?", id);
      return { ms: performance.now() - t0, ops: 4 };
    },
    jsonInsertAndQuery: async (c) => {
      const t0 = performance.now();
      for (let i = 0; i < 50; i++) {
        await c.db.execute(
          "INSERT INTO bench_json (data) VALUES (?)",
          JSON.stringify({
            name: `item-${i}`,
            tags: ["a", "b"],
            score: Math.random() * 100
          })
        );
      }
      const rows = await c.db.execute(
        "SELECT id, json_extract(data, '$.name') as name, json_extract(data, '$.score') as score FROM bench_json ORDER BY json_extract(data, '$.score') DESC LIMIT 10"
      );
      return {
        ms: performance.now() - t0,
        ops: 51,
        rows: rows.length
      };
    },
    jsonEachAgg: async (c) => {
      await c.db.execute(
        "INSERT INTO bench_json (data) VALUES (?)",
        JSON.stringify({
          items: Array.from({ length: 100 }, (_, i) => ({
            id: i,
            val: i * 10
          }))
        })
      );
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT SUM(json_extract(value, '$.val')) as total FROM bench_json, json_each(json_extract(data, '$.items')) LIMIT 1"
      );
      return {
        ms: performance.now() - t0,
        total: rows[0].total
      };
    },
    complexAggregation: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `grp-${i % 10}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT key, COUNT(*) as cnt, AVG(num) as avg_num, MIN(num) as min_num, MAX(num) as max_num FROM bench WHERE key LIKE 'grp-%' GROUP BY key ORDER BY cnt DESC"
      );
      return {
        ms: performance.now() - t0,
        seedMs,
        groups: rows.length
      };
    },
    complexSubquery: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `sq-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT * FROM bench WHERE num > (SELECT AVG(num) FROM bench) ORDER BY num DESC LIMIT 50"
      );
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    complexJoin: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `j-${i}`,
          `v-${i}`,
          i,
          Date.now()
        );
        await c.db.execute(
          "INSERT INTO bench_secondary (bench_id, label, score) VALUES (?, ?, ?)",
          i + 1,
          `label-${i}`,
          Math.random() * 100
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT b.key, b.num, s.label, s.score FROM bench b INNER JOIN bench_secondary s ON s.bench_id = b.id WHERE b.key LIKE 'j-%' ORDER BY s.score DESC LIMIT 200"
      );
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    complexCteWindow: async (c) => {
      const t0Seed = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 200; i++) {
        await c.db.execute(
          "INSERT INTO bench (key, value, num, created_at) VALUES (?, ?, ?, ?)",
          `cte-${i % 10}`,
          `v-${i}`,
          i,
          Date.now()
        );
      }
      await c.db.execute("COMMIT");
      const seedMs = performance.now() - t0Seed;
      const t0 = performance.now();
      const rows = await c.db.execute(`
				WITH ranked AS (
					SELECT key, num, ROW_NUMBER() OVER (PARTITION BY key ORDER BY num DESC) as rn,
					       AVG(num) OVER (PARTITION BY key) as avg_num
					FROM bench
					WHERE key LIKE 'cte-%'
				)
				SELECT * FROM ranked WHERE rn <= 3 ORDER BY key, rn
			`);
      return {
        ms: performance.now() - t0,
        seedMs,
        rows: rows.length
      };
    },
    migrationTables: async (c, n) => {
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < n; i++) {
        await c.db.execute(`CREATE TABLE IF NOT EXISTS mig_${i} (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					data TEXT NOT NULL DEFAULT ''
				)`);
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: n };
    },
    chatLogInsert: async (c, totalBytes) => {
      const t0 = performance.now();
      const seeded = await seedChatLog2(c.db, totalBytes);
      return {
        ms: performance.now() - t0,
        ops: seeded.rows,
        bytes: seeded.totalBytes
      };
    },
    chatLogSelectLimit: async (c, totalBytes) => {
      const seeded = await seedChatLog2(c.db, totalBytes);
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT seq, role, substr(content, 1, 128) AS preview FROM chat_log ORDER BY created_at DESC LIMIT 100"
      );
      return {
        ms: performance.now() - t0,
        ops: rows.length,
        rows: rows.length,
        bytes: seeded.totalBytes
      };
    },
    chatLogSelectIndexed: async (c, totalBytes) => {
      const seeded = await seedChatLog2(c.db, totalBytes);
      const lowerBound = Math.max(0, seeded.rows - 100);
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT seq, role, content_bytes FROM chat_log WHERE thread_id = ? AND seq >= ? ORDER BY seq DESC LIMIT 100",
        seeded.threadId,
        lowerBound
      );
      return {
        ms: performance.now() - t0,
        ops: rows.length,
        rows: rows.length,
        bytes: seeded.totalBytes
      };
    },
    chatLogCount: async (c, totalBytes) => {
      const seeded = await seedChatLog2(c.db, totalBytes);
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT COUNT(*) AS count FROM chat_log WHERE thread_id = ?",
        seeded.threadId
      );
      return {
        ms: performance.now() - t0,
        ops: 1,
        count: rows[0].count,
        bytes: seeded.totalBytes
      };
    },
    chatLogSum: async (c, totalBytes) => {
      const seeded = await seedChatLog2(c.db, totalBytes);
      const t0 = performance.now();
      const rows = await c.db.execute(
        "SELECT SUM(content_bytes) AS total_bytes FROM chat_log WHERE thread_id = ?",
        seeded.threadId
      );
      return {
        ms: performance.now() - t0,
        ops: 1,
        totalBytes: rows[0].total_bytes ?? 0,
        bytes: seeded.totalBytes
      };
    },
    largeTxInsert500KB: async (c) => {
      const targetBytes = 500 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    largeTxInsert1MB: async (c) => {
      const targetBytes = 1024 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    // 1 MiB total, 4096 × 256 B rows. Max NAPI crossings.
    largeTxInsert1MBTinyRows: async (c) => {
      const targetBytes = 1024 * 1024;
      const rowSize = 256;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    // 1 MiB total, 256 × 4 KiB rows. Same shape as largeTxInsert1MB; kept as a sanity duplicate.
    largeTxInsert1MBMediumRows: async (c) => {
      const targetBytes = 1024 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    // 1 MiB total, 1 × 1 MiB row. One NAPI crossing, exercises SQLite overflow-page chain.
    largeTxInsert1MBOneRow: async (c) => {
      const rowSize = 1024 * 1024;
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      await c.db.execute(
        "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
        rowSize
      );
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: 1, bytes: rowSize };
    },
    largeTxInsert5MB: async (c) => {
      const targetBytes = 5 * 1024 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    largeTxInsert10MB: async (c) => {
      const targetBytes = 10 * 1024 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    largeTxInsert50MB: async (c) => {
      const targetBytes = 50 * 1024 * 1024;
      const rowSize = 4 * 1024;
      const rowCount = Math.ceil(targetBytes / rowSize);
      await c.db.execute(`CREATE TABLE IF NOT EXISTS large_tx (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < rowCount; i++) {
        await c.db.execute(
          "INSERT INTO large_tx (payload) VALUES (randomblob(?))",
          rowSize
        );
      }
      await c.db.execute("COMMIT");
      return {
        ms: performance.now() - t0,
        ops: rowCount,
        bytes: rowCount * rowSize
      };
    },
    // Stress test: insert 1000 rows, delete them all, repeat 10 times.
    // Tests freelist reuse and space reclamation patterns.
    churnInsertDelete: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS churn (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      const t0 = performance.now();
      const cycles = 10;
      const perCycle = 1e3;
      for (let cycle = 0; cycle < cycles; cycle++) {
        await c.db.execute("BEGIN");
        for (let i = 0; i < perCycle; i++) {
          await c.db.execute(
            "INSERT INTO churn (payload) VALUES (randomblob(1024))"
          );
        }
        await c.db.execute("DELETE FROM churn");
        await c.db.execute("COMMIT");
      }
      return {
        ms: performance.now() - t0,
        ops: cycles * perCycle,
        cycles
      };
    },
    // Interleave inserts, updates, deletes in same transaction. Tests how
    // the VFS handles mixed page dirtying patterns.
    mixedOltpLarge: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS mixed_oltp (
				id INTEGER PRIMARY KEY,
				value INTEGER NOT NULL,
				data BLOB NOT NULL
			)`);
      await c.db.execute("DELETE FROM mixed_oltp");
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO mixed_oltp (id, value, data) VALUES (?, ?, randomblob(1024))",
          i,
          i * 2
        );
      }
      await c.db.execute("COMMIT");
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO mixed_oltp (id, value, data) VALUES (?, ?, randomblob(1024))",
          500 + i,
          i * 3
        );
        await c.db.execute(
          "UPDATE mixed_oltp SET value = value + 1 WHERE id = ?",
          i
        );
        if (i % 5 === 0) {
          await c.db.execute(
            "DELETE FROM mixed_oltp WHERE id = ?",
            i - 50 >= 0 ? i - 50 : i
          );
        }
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: 500 * 2 + 100 };
    },
    // Growing aggregation: insert then SELECT SUM after each batch.
    // Tests cache invalidation and read-after-write patterns.
    growingAggregation: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS agg_test (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				value INTEGER NOT NULL
			)`);
      await c.db.execute("DELETE FROM agg_test");
      const t0 = performance.now();
      const batches = 20;
      const perBatch = 100;
      let lastSum = 0;
      for (let batch2 = 0; batch2 < batches; batch2++) {
        await c.db.execute("BEGIN");
        for (let i = 0; i < perBatch; i++) {
          await c.db.execute(
            "INSERT INTO agg_test (value) VALUES (?)",
            batch2 * perBatch + i
          );
        }
        await c.db.execute("COMMIT");
        const rows = await c.db.execute(
          "SELECT SUM(value) AS s FROM agg_test"
        );
        lastSum = rows[0]?.s ?? 0;
      }
      return {
        ms: performance.now() - t0,
        ops: batches * perBatch,
        batches,
        lastSum
      };
    },
    // Create index on already-populated table. Tests large rewrite patterns.
    indexCreationOnLargeTable: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS idx_test (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				key TEXT NOT NULL,
				value INTEGER NOT NULL
			)`);
      await c.db.execute("DROP INDEX IF EXISTS idx_test_key");
      await c.db.execute("DELETE FROM idx_test");
      await c.db.execute("BEGIN");
      for (let i = 0; i < 1e4; i++) {
        await c.db.execute(
          "INSERT INTO idx_test (key, value) VALUES (?, ?)",
          `key-${i % 1e3}-${i}`,
          i
        );
      }
      await c.db.execute("COMMIT");
      const t0 = performance.now();
      await c.db.execute("CREATE INDEX idx_test_key ON idx_test(key)");
      return { ms: performance.now() - t0, ops: 1e4 };
    },
    // Update 1000 different rows in separate UPDATEs in one transaction.
    // Stresses B-tree navigation and page dirtying.
    bulkUpdate1000Rows: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS bulk_update (
				id INTEGER PRIMARY KEY,
				value INTEGER NOT NULL
			)`);
      await c.db.execute("DELETE FROM bulk_update");
      await c.db.execute("BEGIN");
      for (let i = 0; i < 1e3; i++) {
        await c.db.execute(
          "INSERT INTO bulk_update (id, value) VALUES (?, ?)",
          i,
          i
        );
      }
      await c.db.execute("COMMIT");
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 1e3; i++) {
        await c.db.execute(
          "UPDATE bulk_update SET value = value + 1 WHERE id = ?",
          i
        );
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: 1e3 };
    },
    // Delete everything then re-insert. Tests truncate+regrow cycle.
    truncateAndRegrow: async (c) => {
      await c.db.execute(`CREATE TABLE IF NOT EXISTS regrow (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				payload BLOB NOT NULL
			)`);
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO regrow (payload) VALUES (randomblob(1024))"
        );
      }
      await c.db.execute("COMMIT");
      const t0 = performance.now();
      await c.db.execute("DELETE FROM regrow");
      await c.db.execute("BEGIN");
      for (let i = 0; i < 500; i++) {
        await c.db.execute(
          "INSERT INTO regrow (payload) VALUES (randomblob(1024))"
        );
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: 500 };
    },
    // Many small tables vs one large. Tests schema page growth.
    manySmallTables: async (c) => {
      const t0 = performance.now();
      await c.db.execute("BEGIN");
      for (let i = 0; i < 50; i++) {
        await c.db.execute(
          `CREATE TABLE IF NOT EXISTS small_t_${i} (id INTEGER PRIMARY KEY, value INTEGER)`
        );
        for (let j = 0; j < 10; j++) {
          await c.db.execute(
            `INSERT INTO small_t_${i} (id, value) VALUES (?, ?)`,
            j,
            i * j
          );
        }
      }
      await c.db.execute("COMMIT");
      return { ms: performance.now() - t0, ops: 50 * 10, tables: 50 };
    }
  }
});

// src/actors/testing/test-sqlite-load.ts
import { actor as actor52 } from "rivetkit";
import { db as db16 } from "rivetkit/db";
var testSqliteLoad = actor52({
  db: db16({
    onMigrate: async (db17) => {
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS schema_version (
					id INTEGER PRIMARY KEY CHECK (id = 1),
					version INTEGER NOT NULL DEFAULT 50
				)
			`);
      await db17.execute(
        "INSERT OR IGNORE INTO schema_version (id, version) VALUES (1, 50)"
      );
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS users (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL,
					email TEXT,
					created_at INTEGER NOT NULL
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS products (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL,
					price REAL NOT NULL DEFAULT 0,
					created_at INTEGER NOT NULL
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS orders (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					total REAL NOT NULL DEFAULT 0,
					status TEXT NOT NULL DEFAULT 'pending',
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS order_items (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id INTEGER NOT NULL,
					product_id INTEGER NOT NULL,
					quantity INTEGER NOT NULL DEFAULT 1,
					price REAL NOT NULL,
					FOREIGN KEY (order_id) REFERENCES orders(id),
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS categories (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL UNIQUE,
					description TEXT
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS product_categories (
					product_id INTEGER NOT NULL,
					category_id INTEGER NOT NULL,
					PRIMARY KEY (product_id, category_id),
					FOREIGN KEY (product_id) REFERENCES products(id),
					FOREIGN KEY (category_id) REFERENCES categories(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS reviews (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					product_id INTEGER NOT NULL,
					rating INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 5),
					comment TEXT,
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id),
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS addresses (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					street TEXT NOT NULL,
					city TEXT NOT NULL,
					state TEXT,
					zip TEXT,
					country TEXT NOT NULL DEFAULT 'US',
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS payments (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id INTEGER NOT NULL,
					amount REAL NOT NULL,
					method TEXT NOT NULL DEFAULT 'card',
					status TEXT NOT NULL DEFAULT 'pending',
					processed_at INTEGER,
					FOREIGN KEY (order_id) REFERENCES orders(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS inventory (
					product_id INTEGER PRIMARY KEY,
					quantity INTEGER NOT NULL DEFAULT 0,
					reserved INTEGER NOT NULL DEFAULT 0,
					last_restocked_at INTEGER,
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS coupons (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					code TEXT NOT NULL UNIQUE,
					discount_percent REAL NOT NULL,
					max_uses INTEGER,
					used_count INTEGER NOT NULL DEFAULT 0,
					expires_at INTEGER
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS shipping (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id INTEGER NOT NULL,
					address_id INTEGER NOT NULL,
					carrier TEXT,
					tracking_number TEXT,
					shipped_at INTEGER,
					delivered_at INTEGER,
					FOREIGN KEY (order_id) REFERENCES orders(id),
					FOREIGN KEY (address_id) REFERENCES addresses(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS tags (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL UNIQUE
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS product_tags (
					product_id INTEGER NOT NULL,
					tag_id INTEGER NOT NULL,
					PRIMARY KEY (product_id, tag_id),
					FOREIGN KEY (product_id) REFERENCES products(id),
					FOREIGN KEY (tag_id) REFERENCES tags(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS wishlists (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					name TEXT NOT NULL DEFAULT 'Default',
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS wishlist_items (
					wishlist_id INTEGER NOT NULL,
					product_id INTEGER NOT NULL,
					added_at INTEGER NOT NULL,
					PRIMARY KEY (wishlist_id, product_id),
					FOREIGN KEY (wishlist_id) REFERENCES wishlists(id),
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS notifications (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					type TEXT NOT NULL,
					message TEXT NOT NULL,
					read INTEGER NOT NULL DEFAULT 0,
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS audit_log (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					entity_type TEXT NOT NULL,
					entity_id INTEGER NOT NULL,
					action TEXT NOT NULL,
					details TEXT,
					performed_at INTEGER NOT NULL
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS sessions (
					id TEXT PRIMARY KEY,
					user_id INTEGER NOT NULL,
					token TEXT NOT NULL UNIQUE,
					expires_at INTEGER NOT NULL,
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_users_email ON users(email)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_orders_user ON orders(user_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_reviews_product ON reviews(product_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_reviews_user ON reviews(user_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_notifications_user ON notifications(user_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_audit_entity ON audit_log(entity_type, entity_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_inventory_quantity ON inventory(quantity)"
      );
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS returns (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					order_id INTEGER NOT NULL,
					reason TEXT NOT NULL,
					status TEXT NOT NULL DEFAULT 'requested',
					requested_at INTEGER NOT NULL,
					processed_at INTEGER,
					FOREIGN KEY (order_id) REFERENCES orders(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS return_items (
					return_id INTEGER NOT NULL,
					order_item_id INTEGER NOT NULL,
					quantity INTEGER NOT NULL,
					PRIMARY KEY (return_id, order_item_id),
					FOREIGN KEY (return_id) REFERENCES returns(id),
					FOREIGN KEY (order_item_id) REFERENCES order_items(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS suppliers (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					name TEXT NOT NULL,
					contact_email TEXT,
					country TEXT
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS product_suppliers (
					product_id INTEGER NOT NULL,
					supplier_id INTEGER NOT NULL,
					cost REAL NOT NULL,
					lead_time_days INTEGER,
					PRIMARY KEY (product_id, supplier_id),
					FOREIGN KEY (product_id) REFERENCES products(id),
					FOREIGN KEY (supplier_id) REFERENCES suppliers(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS price_history (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					product_id INTEGER NOT NULL,
					old_price REAL NOT NULL,
					new_price REAL NOT NULL,
					changed_at INTEGER NOT NULL,
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS user_preferences (
					user_id INTEGER PRIMARY KEY,
					theme TEXT NOT NULL DEFAULT 'dark',
					language TEXT NOT NULL DEFAULT 'en',
					notifications_enabled INTEGER NOT NULL DEFAULT 1,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS cart (
					user_id INTEGER NOT NULL,
					product_id INTEGER NOT NULL,
					quantity INTEGER NOT NULL DEFAULT 1,
					added_at INTEGER NOT NULL,
					PRIMARY KEY (user_id, product_id),
					FOREIGN KEY (user_id) REFERENCES users(id),
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS saved_searches (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					user_id INTEGER NOT NULL,
					query TEXT NOT NULL,
					filters TEXT,
					created_at INTEGER NOT NULL,
					FOREIGN KEY (user_id) REFERENCES users(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS product_images (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					product_id INTEGER NOT NULL,
					url TEXT NOT NULL,
					alt_text TEXT,
					sort_order INTEGER NOT NULL DEFAULT 0,
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS discounts (
					id INTEGER PRIMARY KEY AUTOINCREMENT,
					product_id INTEGER NOT NULL,
					discount_percent REAL NOT NULL,
					starts_at INTEGER NOT NULL,
					ends_at INTEGER,
					FOREIGN KEY (product_id) REFERENCES products(id)
				)
			`);
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_order_items_order ON order_items(order_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_order_items_product ON order_items(product_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_payments_order ON payments(order_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_shipping_order ON shipping(order_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_returns_order ON returns(order_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_price_history_product ON price_history(product_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_product_images_product ON product_images(product_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_discounts_product ON discounts(product_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_cart_user ON cart(user_id)"
      );
      await db17.execute(
        "CREATE INDEX IF NOT EXISTS idx_saved_searches_user ON saved_searches(user_id)"
      );
      await db17.execute(`
				CREATE TABLE IF NOT EXISTS counter (
					id INTEGER PRIMARY KEY CHECK (id = 1),
					value INTEGER NOT NULL DEFAULT 0
				)
			`);
      await db17.execute(
        "INSERT OR IGNORE INTO counter (id, value) VALUES (1, 0)"
      );
    }
  }),
  actions: {
    increment: async (c, amount = 1) => {
      await c.db.execute(
        "UPDATE counter SET value = value + ? WHERE id = 1",
        amount
      );
      const rows = await c.db.execute(
        "SELECT value FROM counter WHERE id = 1"
      );
      return rows[0].value;
    },
    getCount: async (c) => {
      const rows = await c.db.execute(
        "SELECT value FROM counter WHERE id = 1"
      );
      return rows[0].value;
    },
    reset: async (c) => {
      await c.db.execute("UPDATE counter SET value = 0 WHERE id = 1");
      return 0;
    },
    runLoadTest: async (c) => {
      const now = Date.now();
      const results = [];
      await c.db.execute(
        "INSERT INTO users (name, email, created_at) VALUES (?, ?, ?)",
        "Load Test User",
        `load-${now}@test.com`,
        now
      );
      results.push("inserted user");
      const users = await c.db.execute(
        "SELECT * FROM users WHERE email = ?",
        `load-${now}@test.com`
      );
      results.push(
        `fetched user: ${users[0].name}`
      );
      const userId = users[0].id;
      await c.db.execute(
        "INSERT INTO products (name, price, created_at) VALUES (?, ?, ?)",
        "Test Widget",
        29.99,
        now
      );
      results.push("inserted product");
      const products = await c.db.execute(
        "SELECT * FROM products LIMIT 10"
      );
      results.push(`fetched ${products.length} products`);
      const productId = products[0].id;
      await c.db.execute(
        "INSERT OR IGNORE INTO categories (name, description) VALUES (?, ?)",
        `test-cat-${now}`,
        "A test category"
      );
      results.push("inserted category");
      const categories = await c.db.execute("SELECT * FROM categories");
      results.push(
        `fetched ${categories.length} categories`
      );
      await c.db.execute(
        "INSERT INTO orders (user_id, total, status, created_at) VALUES (?, ?, ?, ?)",
        userId,
        29.99,
        "pending",
        now
      );
      results.push("inserted order");
      const orders = await c.db.execute(
        "SELECT * FROM orders WHERE user_id = ?",
        userId
      );
      results.push(
        `fetched ${orders.length} orders for user`
      );
      const orderId = orders[0].id;
      await c.db.execute(
        "INSERT INTO order_items (order_id, product_id, quantity, price) VALUES (?, ?, ?, ?)",
        orderId,
        productId,
        2,
        29.99
      );
      results.push("inserted order item");
      await c.db.execute(
        "INSERT OR REPLACE INTO inventory (product_id, quantity, reserved, last_restocked_at) VALUES (?, ?, ?, ?)",
        productId,
        100,
        2,
        now
      );
      results.push("inserted inventory");
      await c.db.execute(
        "INSERT INTO reviews (user_id, product_id, rating, comment, created_at) VALUES (?, ?, ?, ?, ?)",
        userId,
        productId,
        5,
        "Great product!",
        now
      );
      results.push("inserted review");
      const reviews = await c.db.execute(
        "SELECT r.*, u.name as reviewer FROM reviews r JOIN users u ON r.user_id = u.id WHERE r.product_id = ?",
        productId
      );
      results.push(`fetched ${reviews.length} reviews`);
      await c.db.execute(
        "INSERT INTO notifications (user_id, type, message, created_at) VALUES (?, ?, ?, ?)",
        userId,
        "order",
        "Your order has been placed",
        now
      );
      results.push("inserted notification");
      await c.db.execute(
        "INSERT INTO audit_log (entity_type, entity_id, action, details, performed_at) VALUES (?, ?, ?, ?, ?)",
        "order",
        orderId,
        "created",
        `Order created by user ${userId}`,
        now
      );
      results.push("inserted audit log");
      const orderStats = await c.db.execute(
        "SELECT status, COUNT(*) as count, SUM(total) as total_value FROM orders GROUP BY status"
      );
      results.push(
        `order stats: ${orderStats.length} statuses`
      );
      await c.db.execute(
        "INSERT INTO addresses (user_id, street, city, state, zip, country) VALUES (?, ?, ?, ?, ?, ?)",
        userId,
        "123 Test St",
        "Testville",
        "CA",
        "90210",
        "US"
      );
      results.push("inserted address");
      const orderDetails = await c.db.execute(`
				SELECT o.id, o.status, o.total, u.name as customer, COUNT(oi.id) as item_count
				FROM orders o
				JOIN users u ON o.user_id = u.id
				LEFT JOIN order_items oi ON oi.order_id = o.id
				GROUP BY o.id
				LIMIT 10
			`);
      results.push(
        `fetched ${orderDetails.length} order details`
      );
      await c.db.execute(
        "UPDATE orders SET status = ? WHERE id = ?",
        "completed",
        orderId
      );
      results.push("updated order status");
      const version = await c.db.execute(
        "SELECT version FROM schema_version WHERE id = 1"
      );
      results.push(
        `schema version: ${version[0].version}`
      );
      const tableCounts = await c.db.execute(`
				SELECT 'users' as tbl, COUNT(*) as cnt FROM users
				UNION ALL SELECT 'products', COUNT(*) FROM products
				UNION ALL SELECT 'orders', COUNT(*) FROM orders
				UNION ALL SELECT 'reviews', COUNT(*) FROM reviews
				UNION ALL SELECT 'categories', COUNT(*) FROM categories
			`);
      results.push(
        `table counts: ${tableCounts.length} tables checked`
      );
      return {
        queriesRun: 20,
        results
      };
    }
  }
});

// src/actors/workflow/approval.ts
import { actor as actor53, event as event11, queue as queue4 } from "rivetkit";
import { Loop, workflow } from "rivetkit/workflow";
var QUEUE_DECISION = "decision";
var APPROVAL_TIMEOUT_MS = 3e4;
var approval = actor53({
  createState: (c, input) => ({
    id: c.key[0],
    title: input?.title ?? "Untitled Request",
    description: input?.description ?? "",
    status: "pending",
    createdAt: Date.now()
  }),
  queues: {
    decision: queue4()
  },
  events: {
    requestUpdated: event11(),
    requestCreated: event11()
  },
  actions: {
    getRequest: (c) => c.state,
    approve: async (c, approver) => {
      if (c.state.status !== "pending") return;
      c.state.deciding = true;
      c.broadcast("requestUpdated", c.state);
      await c.queue.send(QUEUE_DECISION, { approved: true, approver });
    },
    reject: async (c, approver) => {
      if (c.state.status !== "pending") return;
      c.state.deciding = true;
      c.broadcast("requestUpdated", c.state);
      await c.queue.send(QUEUE_DECISION, { approved: false, approver });
    }
  },
  run: workflow(async (ctx) => {
    await ctx.loop("approval-loop", async (loopCtx) => {
      await loopCtx.step("init-request", async (c) => {
        c.log.info({
          msg: "waiting for approval decision",
          requestId: c.state.id,
          title: c.state.title
        });
        c.broadcast("requestCreated", c.state);
      });
      const [decisionMessage] = await loopCtx.queue.nextBatch(
        "wait-decision",
        {
          names: [QUEUE_DECISION],
          timeout: APPROVAL_TIMEOUT_MS
        }
      );
      const decision = decisionMessage?.body ?? null;
      await loopCtx.step("update-status", async (c) => {
        c.state.deciding = false;
        if (decision === null) {
          c.state.status = "timeout";
          c.log.info({
            msg: "request timed out",
            requestId: c.state.id
          });
        } else if (decision.approved) {
          c.state.status = "approved";
          c.state.decidedBy = decision.approver;
          c.log.info({
            msg: "request approved",
            requestId: c.state.id,
            approver: decision.approver
          });
        } else {
          c.state.status = "rejected";
          c.state.decidedBy = decision.approver;
          c.log.info({
            msg: "request rejected",
            requestId: c.state.id,
            approver: decision.approver
          });
        }
        c.state.decidedAt = Date.now();
        c.broadcast("requestUpdated", c.state);
      });
      return Loop.break(void 0);
    });
  })
});

// src/actors/workflow/batch.ts
import { actor as actor54, event as event12 } from "rivetkit";
import { Loop as Loop2, workflow as workflow2 } from "rivetkit/workflow";
function fetchBatch(cursor, batchSize, totalItems) {
  const start = cursor * batchSize;
  const end = Math.min(start + batchSize, totalItems);
  const items = [];
  for (let i = start; i < end; i++) {
    items.push(i);
  }
  return {
    items,
    hasMore: end < totalItems
  };
}
var batch = actor54({
  createState: (c, input) => ({
    id: c.key[0],
    totalItems: input?.totalItems ?? 50,
    batchSize: input?.batchSize ?? 5,
    status: "running",
    processedTotal: 0,
    currentBatch: 0,
    batches: [],
    startedAt: Date.now()
  }),
  events: {
    batchProcessed: event12(),
    stateChanged: event12(),
    processingComplete: event12()
  },
  actions: {
    getJob: (c) => c.state
  },
  run: workflow2(async (ctx) => {
    await ctx.loop({
      name: "batch-loop",
      state: { cursor: 0 },
      run: async (batchCtx, loopState) => {
        const batch2 = await batchCtx.step("fetch-batch", async (c) => {
          c.log.info({
            msg: "processing batch",
            jobId: c.state.id,
            cursor: loopState.cursor
          });
          await new Promise(
            (r) => setTimeout(r, 200 + Math.random() * 300)
          );
          return fetchBatch(
            loopState.cursor,
            c.state.batchSize,
            c.state.totalItems
          );
        });
        await batchCtx.step("process-batch", async (c) => {
          await new Promise(
            (r) => setTimeout(r, 300 + Math.random() * 500)
          );
          const batchInfo = {
            id: loopState.cursor,
            count: batch2.items.length,
            processedAt: Date.now()
          };
          c.state.currentBatch = loopState.cursor;
          c.state.processedTotal += batch2.items.length;
          c.state.batches.push(batchInfo);
          c.broadcast("batchProcessed", batchInfo);
          c.broadcast("stateChanged", c.state);
          c.log.info({
            msg: "batch processed",
            jobId: c.state.id,
            cursor: loopState.cursor,
            count: batch2.items.length
          });
        });
        if (!batch2.hasMore) {
          await batchCtx.step("mark-complete", async (c) => {
            c.state.status = "completed";
            c.state.completedAt = Date.now();
            c.broadcast("stateChanged", c.state);
            c.broadcast("processingComplete", {
              totalBatches: loopState.cursor + 1,
              totalItems: c.state.processedTotal
            });
          });
          return Loop2.break(loopState.cursor + 1);
        }
        return Loop2.continue({ cursor: loopState.cursor + 1 });
      }
    });
  })
});

// src/actors/workflow/dashboard.ts
import { actor as actor55, event as event13, queue as queue5 } from "rivetkit";
import { Loop as Loop3, workflow as workflow3 } from "rivetkit/workflow";
var QUEUE_REFRESH = "refresh";
async function fetchUserStats() {
  await new Promise((r) => setTimeout(r, 800 + Math.random() * 1200));
  return {
    count: Math.floor(1e3 + Math.random() * 500),
    activeToday: Math.floor(100 + Math.random() * 200),
    newThisWeek: Math.floor(20 + Math.random() * 80)
  };
}
async function fetchOrderStats() {
  await new Promise((r) => setTimeout(r, 600 + Math.random() * 1e3));
  const count = Math.floor(50 + Math.random() * 150);
  const revenue = Math.floor(5e3 + Math.random() * 15e3);
  return {
    count,
    revenue,
    avgOrderValue: Math.round(revenue / count)
  };
}
async function fetchMetricsStats() {
  await new Promise((r) => setTimeout(r, 400 + Math.random() * 800));
  return {
    pageViews: Math.floor(1e4 + Math.random() * 5e4),
    sessions: Math.floor(2e3 + Math.random() * 8e3),
    bounceRate: Math.round(30 + Math.random() * 40)
  };
}
var dashboard = actor55({
  state: {
    data: null,
    loading: false,
    branches: {
      users: "pending",
      orders: "pending",
      metrics: "pending"
    },
    lastRefresh: null
  },
  queues: {
    [QUEUE_REFRESH]: queue5()
  },
  events: {
    stateChanged: event13(),
    refreshComplete: event13()
  },
  actions: {
    refresh: async (c) => {
      if (!c.state.loading) {
        c.state.loading = true;
        c.state.branches = {
          users: "pending",
          orders: "pending",
          metrics: "pending"
        };
        c.broadcast("stateChanged", c.state);
        await c.queue.send(QUEUE_REFRESH, {});
      }
    },
    getState: (c) => c.state
  },
  run: workflow3(async (ctx) => {
    await ctx.loop("refresh-loop", async (loopCtx) => {
      await loopCtx.queue.next("wait-refresh", {
        names: [QUEUE_REFRESH]
      });
      ctx.log.info({ msg: "starting dashboard refresh" });
      const results = await loopCtx.join("fetch-all", {
        users: {
          run: async (branchCtx) => {
            await branchCtx.step("mark-running", async (bc) => {
              bc.state.branches.users = "running";
              bc.broadcast("stateChanged", bc.state);
            });
            const data = await branchCtx.step(
              "fetch-users",
              async () => {
                return await fetchUserStats();
              }
            );
            await branchCtx.step("mark-complete", async (bc) => {
              bc.state.branches.users = "completed";
              bc.broadcast("stateChanged", bc.state);
            });
            return data;
          }
        },
        orders: {
          run: async (branchCtx) => {
            await branchCtx.step("mark-running", async (bc) => {
              bc.state.branches.orders = "running";
              bc.broadcast("stateChanged", bc.state);
            });
            const data = await branchCtx.step(
              "fetch-orders",
              async () => {
                return await fetchOrderStats();
              }
            );
            await branchCtx.step("mark-complete", async (bc) => {
              bc.state.branches.orders = "completed";
              bc.broadcast("stateChanged", bc.state);
            });
            return data;
          }
        },
        metrics: {
          run: async (branchCtx) => {
            await branchCtx.step("mark-running", async (bc) => {
              bc.state.branches.metrics = "running";
              bc.broadcast("stateChanged", bc.state);
            });
            const data = await branchCtx.step(
              "fetch-metrics",
              async () => {
                return await fetchMetricsStats();
              }
            );
            await branchCtx.step("mark-complete", async (bc) => {
              bc.state.branches.metrics = "completed";
              bc.broadcast("stateChanged", bc.state);
            });
            return data;
          }
        }
      });
      await loopCtx.step("save-data", async (c) => {
        c.state.data = {
          users: results.users,
          orders: results.orders,
          metrics: results.metrics,
          fetchedAt: Date.now()
        };
        c.state.loading = false;
        c.state.lastRefresh = Date.now();
        c.broadcast("stateChanged", c.state);
        c.broadcast("refreshComplete", c.state.data);
      });
      ctx.log.info({ msg: "dashboard refresh complete" });
      return Loop3.continue(void 0);
    });
  })
});

// src/actors/workflow/history-examples.ts
import { actor as actor56, queue as queue6 } from "rivetkit";
import { Loop as Loop4, workflow as workflow4 } from "rivetkit/workflow";
function delay3(ms) {
  return new Promise((resolve2) => setTimeout(resolve2, ms));
}
var workflowHistorySimple = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "pending"
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("start", async (c) => {
      c.state.status = "running";
      c.state.lastStep = "start";
      c.state.startedAt = Date.now();
      return { initialized: true };
    });
    await delay3(700);
    await ctx.step("process", async (c) => {
      c.state.lastStep = "process";
      return { processed: true, items: 5 };
    });
    await delay3(2200);
    await ctx.step("validate", async (c) => {
      c.state.lastStep = "validate";
      return { valid: true };
    });
    await delay3(600);
    await ctx.step("complete", async (c) => {
      c.state.lastStep = "complete";
      c.state.status = "completed";
      c.state.completedAt = Date.now();
      c.state.output = { success: true, processedItems: 3 };
      return { success: true };
    });
  })
});
var LOOP_ITEMS = ["A", "B", "C"];
var workflowHistoryLoop = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "running",
    processed: 0,
    batches: []
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("init", async (c) => {
      c.state.status = "running";
      return { batchSize: LOOP_ITEMS.length };
    });
    await ctx.loop({
      name: "batch-loop",
      state: { index: 0 },
      commitInterval: 1,
      historyEvery: 1,
      historyKeep: LOOP_ITEMS.length,
      run: async (loopCtx, loopState) => {
        const item = LOOP_ITEMS[loopState.index];
        await loopCtx.step(`process-${loopState.index}`, async (c) => {
          c.state.processed += 1;
          c.state.batches.push({ index: loopState.index, item });
          return { item, status: "done" };
        });
        if (loopState.index >= LOOP_ITEMS.length - 1) {
          return Loop4.break({ processed: LOOP_ITEMS.length });
        }
        return Loop4.continue({ index: loopState.index + 1 });
      }
    });
    await ctx.step("finalize", async (c) => {
      c.state.status = "completed";
      c.state.completedAt = Date.now();
      return { allProcessed: true };
    });
  })
});
var workflowHistoryJoin = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "pending"
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("start", async (c) => {
      c.state.status = "running";
      return { ready: true };
    });
    const results = await ctx.join("parallel-tasks", {
      "fetch-api": {
        run: async (branchCtx) => {
          await branchCtx.step("task-a", async (branchCtx2) => {
            await delay3(120);
            return { fetched: true };
          });
          return { data: "api-response" };
        }
      },
      "query-db": {
        run: async (branchCtx) => {
          await branchCtx.step("task-b", async (branchCtx2) => {
            await delay3(200);
            return { queried: true };
          });
          return { rows: 42 };
        }
      },
      "check-cache": {
        run: async (branchCtx) => {
          await branchCtx.step("task-c", async (branchCtx2) => {
            await delay3(60);
            return { checked: true };
          });
          return { hit: true };
        }
      }
    });
    await ctx.step("merge-results", async (c) => {
      c.state.status = "completed";
      c.state.result = {
        api: results["fetch-api"].data,
        rows: results["query-db"].rows,
        cacheHit: results["check-cache"].hit
      };
      return { merged: true };
    });
  })
});
var workflowHistoryRace = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "running"
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("begin", async (c) => {
      c.state.status = "running";
      return { started: true };
    });
    const { winner, value } = await ctx.race("race-providers", [
      {
        name: "provider-fast",
        run: async (branchCtx) => {
          await branchCtx.sleep("provider-fast-step", 50);
          return { provider: "cdn-edge", latency: 12 };
        }
      },
      {
        name: "provider-slow",
        run: async (branchCtx) => {
          await branchCtx.sleep("provider-slow-step", 200);
          return { provider: "origin", latency: 120 };
        }
      }
    ]);
    await ctx.step("use-result", async (c) => {
      c.state.status = "completed";
      c.state.winner = winner;
      c.state.result = value.provider;
      return { used: value.provider };
    });
  })
});
var QUEUE_ORDER_CREATED = "order:created";
var QUEUE_ORDER_UPDATED = "order:updated";
var QUEUE_ORDER_ITEM = "order:item";
var QUEUE_ORDER_ARTIFACT = "order:artifact";
var QUEUE_ORDER_READY = "order:ready";
var QUEUE_ORDER_OPTIONAL = "order:optional";
var FULL_WORKFLOW_MESSAGE_SEEDS = [
  { name: QUEUE_ORDER_CREATED, payload: { id: "order-1" } },
  { name: QUEUE_ORDER_UPDATED, payload: { id: "order-1", status: "paid" } },
  { name: QUEUE_ORDER_ITEM, payload: { sku: "sku-0", qty: 1 } },
  { name: QUEUE_ORDER_ITEM, payload: { sku: "sku-4", qty: 1 } },
  { name: QUEUE_ORDER_ARTIFACT, payload: { artifactId: "artifact-0" } },
  { name: QUEUE_ORDER_ARTIFACT, payload: { artifactId: "artifact-1" } },
  { name: QUEUE_ORDER_ARTIFACT, payload: { artifactId: "artifact-2" } },
  { name: QUEUE_ORDER_READY, payload: { batch: 3 } },
  { name: QUEUE_ORDER_READY, payload: { batch: 0 } },
  { name: QUEUE_ORDER_READY, payload: { batch: 2 } }
];
var FULL_WORKFLOW_ITEMS = [
  { id: "item-1", basePrice: 100, tax: 8 },
  { id: "item-2", basePrice: 115, tax: 9 },
  { id: "item-3", basePrice: 130, tax: 10 },
  { id: "item-4", basePrice: 145, tax: 12 }
];
var workflowHistoryFull = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "pending",
    seededMessages: false
  }),
  queues: {
    [QUEUE_ORDER_CREATED]: queue6(),
    [QUEUE_ORDER_UPDATED]: queue6(),
    [QUEUE_ORDER_ITEM]: queue6(),
    [QUEUE_ORDER_ARTIFACT]: queue6(),
    [QUEUE_ORDER_READY]: queue6(),
    [QUEUE_ORDER_OPTIONAL]: queue6()
  },
  actions: {
    getState: (c) => c.state,
    seedMessages: async (c) => {
      if (c.state.seededMessages) return;
      for (const seed of FULL_WORKFLOW_MESSAGE_SEEDS) {
        await c.queue.send(seed.name, seed.payload);
      }
      c.state.seededMessages = true;
    }
  },
  run: workflow4(async (ctx) => {
    await ctx.step("bootstrap", async (c) => {
      c.state.status = "running";
      c.state.lastStep = "bootstrap";
      c.state.startedAt = Date.now();
      return {
        requestId: `req-${c.state.id}`,
        startedAt: Date.now()
      };
    });
    await ctx.step("validate-input", async (c) => {
      c.state.lastStep = "validate-input";
      return true;
    });
    await ctx.rollbackCheckpoint("checkpoint-after-validation");
    await ctx.step("load-user-profile", async (c) => {
      c.state.lastStep = "load-user-profile";
      return {
        id: "user-123",
        tier: "standard",
        flags: ["email-verified", "promo-eligible"]
      };
    });
    await ctx.step("compute-discount", async (c) => {
      c.state.lastStep = "compute-discount";
      return { percent: 5, reason: "tier-discount" };
    });
    await ctx.step("ephemeral-cache-check", async (c) => {
      c.state.lastStep = "ephemeral-cache-check";
      return { cacheHit: false, tier: "standard" };
    });
    await ctx.rollbackCheckpoint("checkpoint-before-reserve");
    await ctx.loop({
      name: "process-items-loop",
      state: { index: 0 },
      commitInterval: 1,
      historyEvery: 1,
      historyKeep: 2,
      run: async (loopCtx, loopState) => {
        const item = FULL_WORKFLOW_ITEMS[loopState.index];
        if (!item) {
          return Loop4.break({ count: FULL_WORKFLOW_ITEMS.length });
        }
        await loopCtx.step(
          `fetch-item-${loopState.index}`,
          async (loopCtx2) => {
            return { itemId: item.id, basePrice: item.basePrice };
          }
        );
        await loopCtx.step(
          `compute-tax-${loopState.index}`,
          async (loopCtx2) => {
            return item.tax;
          }
        );
        await loopCtx.step(
          `reserve-inventory-${loopState.index}`,
          async (loopCtx2) => ({
            reservationId: `res-${loopState.index}`,
            itemId: item.id
          })
        );
        if (loopState.index >= FULL_WORKFLOW_ITEMS.length - 1) {
          return Loop4.break({
            count: FULL_WORKFLOW_ITEMS.length,
            total: 504
          });
        }
        return Loop4.continue({ index: loopState.index + 1 });
      }
    });
    await ctx.sleep("short-cooldown", 40);
    await ctx.sleep("cooldown-sleep", 60);
    await ctx.sleep("wait-until-deadline", 45);
    await ctx.step("compute-deadlines", async (ctx2) => {
      const readyBy = Date.now() + 800;
      const readyBatchBy = Date.now() + 1100;
      return { readyBy, readyBatchBy };
    });
    await ctx.queue.next("listen-order-created", {
      names: [QUEUE_ORDER_CREATED]
    });
    await ctx.queue.nextBatch("listen-order-updated-timeout", {
      names: [QUEUE_ORDER_UPDATED],
      timeout: 250
    });
    await ctx.queue.nextBatch("listen-batch-two", {
      names: [QUEUE_ORDER_ITEM],
      count: 2
    });
    await ctx.queue.nextBatch("listen-artifacts-timeout", {
      names: [QUEUE_ORDER_ARTIFACT],
      count: 3,
      timeout: 300
    });
    await ctx.queue.nextBatch("listen-optional", {
      names: [QUEUE_ORDER_OPTIONAL],
      timeout: 200
    });
    await ctx.queue.nextBatch("listen-until", {
      names: [QUEUE_ORDER_READY],
      timeout: 300
    });
    await ctx.queue.nextBatch("listen-batch-until", {
      names: [QUEUE_ORDER_READY],
      count: 2,
      timeout: 400
    });
    await ctx.join("join-dependencies", {
      inventory: {
        run: async (branchCtx) => {
          const reserved = await branchCtx.step(
            "inventory-audit",
            async (branchCtx2) => 4
          );
          await branchCtx.sleep("join-inventory-sleep", 35);
          return {
            reserved,
            checked: 4,
            notes: ["inventory-ok", "items=4"]
          };
        }
      },
      pricing: {
        run: async (branchCtx) => {
          const method = await branchCtx.step(
            "pricing-method",
            async (branchCtx2) => "promo"
          );
          return {
            subtotal: 504,
            discount: 25,
            total: 479,
            method
          };
        }
      },
      shipping: {
        run: async (branchCtx) => {
          const zone = await branchCtx.step(
            "shipping-zone",
            async (branchCtx2) => "us-east"
          );
          await branchCtx.sleep("join-shipping-sleep", 35);
          return { method: "ground", etaDays: 4, zone };
        }
      }
    });
    await ctx.race("race-fulfillment", [
      {
        name: "race-fast",
        run: async (branchCtx) => {
          await branchCtx.sleep("race-fast-sleep", 70);
          return { method: "express", cost: 18, etaDays: 1 };
        }
      },
      {
        name: "race-slow",
        run: async (branchCtx) => {
          await branchCtx.sleep("race-slow-sleep", 250);
          return { method: "ground", cost: 8, etaDays: 4 };
        }
      }
    ]);
    await ctx.removed("legacy-step-placeholder", "step");
    await ctx.step("finalize", async (c) => {
      c.state.lastStep = "finalize";
      c.state.status = "completed";
      c.state.completedAt = Date.now();
      return true;
    });
  })
});
var workflowHistoryInProgress = actor56({
  createState: (c, input) => ({
    id: c.key[0],
    status: "running",
    processingDurationMs: input?.processingDurationMs ?? 3e4,
    progress: 0
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("init", async (c) => {
      c.state.startedAt = Date.now();
      c.state.progress = 10;
      return { initialized: true };
    });
    await ctx.step("fetch-data", async (c) => {
      c.state.progress = 25;
      return { fetched: true, records: 100 };
    });
    await ctx.step("process", async (c) => {
      c.state.progress = 42;
      await delay3(c.state.processingDurationMs);
      c.state.status = "completed";
      c.state.completedAt = Date.now();
      return { processed: true };
    });
  })
});
var RETRY_MAX_RETRIES = 20;
var workflowHistoryRetrying = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "running",
    attempts: 0,
    succeedAfter: 999
  }),
  actions: {
    getState: (c) => c.state,
    allowSuccess: (c, afterAttempt) => {
      c.state.succeedAfter = afterAttempt ?? c.state.attempts + 1;
    }
  },
  run: workflow4(async (ctx) => {
    await ctx.step("start", async (c) => {
      c.state.status = "running";
      return { ready: true };
    });
    await ctx.step({
      name: "api-call",
      maxRetries: RETRY_MAX_RETRIES,
      retryBackoffBase: 250,
      retryBackoffMax: 1500,
      run: async (c) => {
        c.state.attempts += 1;
        if (c.state.attempts < c.state.succeedAfter) {
          const error = new Error("Connection timeout after 5000ms");
          c.state.lastError = error.message;
          throw error;
        }
        c.state.status = "completed";
        c.state.lastError = void 0;
        return { success: true };
      }
    });
  })
});
var FAILED_MAX_RETRIES = 3;
var workflowHistoryFailed = actor56({
  createState: (c) => ({
    id: c.key[0],
    status: "running",
    attempts: 0
  }),
  actions: {
    getState: (c) => c.state
  },
  run: workflow4(async (ctx) => {
    await ctx.step("init", async (c) => {
      c.state.status = "running";
      return { initialized: true };
    });
    await ctx.step("validate", async (ctx2) => {
      return { valid: true };
    });
    await ctx.step({
      name: "process",
      maxRetries: FAILED_MAX_RETRIES,
      retryBackoffBase: 200,
      retryBackoffMax: 800,
      run: async (c) => {
        c.state.attempts += 1;
        const error = new Error(
          "Database connection failed: ECONNREFUSED"
        );
        c.state.lastError = error.message;
        throw error;
      }
    });
  })
});

// src/actors/workflow/order.ts
import { actor as actor57, event as event14 } from "rivetkit";
import { Loop as Loop5, workflow as workflow5 } from "rivetkit/workflow";
async function simulateWork(name, failChance = 0.1) {
  await new Promise(
    (resolve2) => setTimeout(resolve2, 500 + Math.random() * 1e3)
  );
  if (Math.random() < failChance) {
    throw new Error(`${name} failed (simulated)`);
  }
}
var order = actor57({
  createState: (c) => ({
    id: c.key[0],
    status: "pending",
    step: 0,
    createdAt: Date.now()
  }),
  events: {
    orderUpdated: event14()
  },
  actions: {
    getOrder: (c) => c.state
  },
  run: workflow5(async (ctx) => {
    await ctx.loop("process-order", async (loopCtx) => {
      await loopCtx.step("validate", async (c) => {
        c.log.info({
          msg: "processing order",
          orderId: c.state.id
        });
        c.state.status = "validating";
        c.state.step = 1;
        c.broadcast("orderUpdated", c.state);
        await simulateWork("validation", 0.05);
      });
      await loopCtx.step("charge", async (c) => {
        c.state.status = "charging";
        c.state.step = 2;
        c.broadcast("orderUpdated", c.state);
        await simulateWork("payment", 0.1);
      });
      await loopCtx.step("fulfill", async (c) => {
        c.state.status = "fulfilling";
        c.state.step = 3;
        c.broadcast("orderUpdated", c.state);
        await simulateWork("fulfillment", 0.05);
      });
      await loopCtx.step("complete", async (c) => {
        c.state.status = "completed";
        c.state.step = 4;
        c.state.completedAt = Date.now();
        c.broadcast("orderUpdated", c.state);
        c.log.info({
          msg: "order completed",
          orderId: c.state.id
        });
      });
      return Loop5.break(void 0);
    });
  })
});

// src/actors/workflow/payment.ts
import { actor as actor58, event as event15 } from "rivetkit";
import { Loop as Loop6, workflow as workflow6 } from "rivetkit/workflow";
var payment = actor58({
  createState: (c, input) => ({
    id: c.key[0],
    amount: input?.amount ?? 100,
    shouldFail: input?.shouldFail ?? false,
    status: "pending",
    steps: [
      { name: "reserve-inventory", status: "pending" },
      { name: "charge-card", status: "pending" },
      { name: "complete-order", status: "pending" }
    ],
    startedAt: Date.now()
  }),
  events: {
    transactionStarted: event15(),
    transactionUpdated: event15(),
    transactionCompleted: event15()
  },
  actions: {
    getTransaction: (c) => c.state
  },
  run: workflow6(async (ctx) => {
    await ctx.loop("payment-loop", async (loopCtx) => {
      await loopCtx.step("init-payment", async (c) => {
        c.log.info({
          msg: "starting payment processing",
          txId: c.state.id,
          amount: c.state.amount,
          shouldFail: c.state.shouldFail
        });
        c.broadcast("transactionStarted", c.state);
      });
      await loopCtx.rollbackCheckpoint("payment-checkpoint");
      await loopCtx.step({
        name: "reserve-inventory",
        run: async (c) => {
          c.state.status = "reserving";
          const step = c.state.steps.find(
            (s) => s.name === "reserve-inventory"
          );
          if (step) {
            step.status = "completed";
            step.completedAt = Date.now();
          }
          c.broadcast("transactionUpdated", c.state);
          await new Promise(
            (r) => setTimeout(r, 500 + Math.random() * 500)
          );
          c.log.info({
            msg: "inventory reserved",
            txId: c.state.id
          });
          return { reserved: true };
        },
        rollback: async (c) => {
          c.state.status = "rolling_back";
          const step = c.state.steps.find(
            (s) => s.name === "reserve-inventory"
          );
          if (step) {
            step.status = "rolled_back";
            step.rolledBackAt = Date.now();
          }
          c.log.info({
            msg: "inventory released",
            txId: c.state.id
          });
          c.broadcast("transactionUpdated", c.state);
          await new Promise((r) => setTimeout(r, 400));
        }
      });
      await loopCtx.step({
        name: "charge-card",
        run: async (c) => {
          c.state.status = "charging";
          const step = c.state.steps.find(
            (s) => s.name === "charge-card"
          );
          if (step) {
            step.status = "completed";
            step.completedAt = Date.now();
          }
          c.broadcast("transactionUpdated", c.state);
          await new Promise(
            (r) => setTimeout(r, 500 + Math.random() * 500)
          );
          if (c.state.shouldFail) {
            throw new Error("Payment declined (simulated)");
          }
          c.log.info({ msg: "card charged", txId: c.state.id });
          return { chargeId: `ch_${c.state.id}` };
        },
        rollback: async (c) => {
          c.state.status = "rolling_back";
          const step = c.state.steps.find(
            (s) => s.name === "charge-card"
          );
          if (step) {
            step.status = "rolled_back";
            step.rolledBackAt = Date.now();
          }
          c.log.info({ msg: "charge refunded", txId: c.state.id });
          c.broadcast("transactionUpdated", c.state);
          await new Promise((r) => setTimeout(r, 400));
        }
      });
      await loopCtx.step("complete-order", async (c) => {
        c.state.status = "completing";
        const step = c.state.steps.find(
          (s) => s.name === "complete-order"
        );
        if (step) step.status = "completed";
        c.broadcast("transactionUpdated", c.state);
        await new Promise(
          (r) => setTimeout(r, 300 + Math.random() * 300)
        );
        c.state.status = "completed";
        c.state.completedAt = Date.now();
        c.log.info({ msg: "order completed", txId: c.state.id });
        c.broadcast("transactionCompleted", c.state);
      });
      return Loop6.break(void 0);
    });
  })
});

// src/actors/workflow/race.ts
import { actor as actor59, event as event16 } from "rivetkit";
import { Loop as Loop7, workflow as workflow7 } from "rivetkit/workflow";
var race = actor59({
  createState: (c, input) => ({
    id: c.key[0],
    workDurationMs: input?.workDurationMs ?? 2e3,
    timeoutMs: input?.timeoutMs ?? 3e3,
    status: "running",
    startedAt: Date.now()
  }),
  events: {
    raceStarted: event16(),
    raceCompleted: event16()
  },
  actions: {
    getTask: (c) => c.state
  },
  run: workflow7(async (ctx) => {
    await ctx.loop("race-loop", async (loopCtx) => {
      const { workDurationMs, timeoutMs, taskId } = await loopCtx.step(
        "start-race",
        async (c) => {
          c.log.info({
            msg: "starting race",
            taskId: c.state.id,
            workDurationMs: c.state.workDurationMs,
            timeoutMs: c.state.timeoutMs
          });
          c.broadcast("raceStarted", c.state);
          return {
            workDurationMs: c.state.workDurationMs,
            timeoutMs: c.state.timeoutMs,
            taskId: c.state.id
          };
        }
      );
      const { winner, value } = await loopCtx.race("work-vs-timeout", [
        {
          name: "work",
          run: async (branchCtx) => {
            await branchCtx.sleep("simulate-work", workDurationMs);
            return await branchCtx.step(
              "complete-work",
              async () => {
                return `Result for task ${taskId}`;
              }
            );
          }
        },
        {
          name: "timeout",
          run: async (branchCtx) => {
            await branchCtx.sleep("timeout-wait", timeoutMs);
            return null;
          }
        }
      ]);
      await loopCtx.step("save-result", async (c) => {
        c.state.completedAt = Date.now();
        c.state.actualDurationMs = c.state.completedAt - c.state.startedAt;
        if (winner === "work") {
          c.state.status = "work_won";
          c.state.result = value;
          c.log.info({
            msg: "work completed before timeout",
            taskId: c.state.id,
            durationMs: c.state.actualDurationMs
          });
        } else {
          c.state.status = "timeout_won";
          c.log.info({
            msg: "timeout won the race",
            taskId: c.state.id,
            durationMs: c.state.actualDurationMs
          });
        }
        c.broadcast("raceCompleted", c.state);
      });
      return Loop7.break(void 0);
    });
  })
});

// src/actors/workflow/timer.ts
import { actor as actor60, event as event17 } from "rivetkit";
import { Loop as Loop8, workflow as workflow8 } from "rivetkit/workflow";
var timer = actor60({
  createState: (c, input) => ({
    id: c.key[0],
    name: input?.name ?? "Timer",
    durationMs: input?.durationMs ?? 1e4,
    startedAt: Date.now()
  }),
  events: {
    timerStarted: event17(),
    timerCompleted: event17()
  },
  actions: {
    getTimer: (c) => c.state
  },
  run: workflow8(async (ctx) => {
    await ctx.loop("timer-loop", async (loopCtx) => {
      const durationMs = await loopCtx.step("start-timer", async (c) => {
        c.log.info({
          msg: "starting timer",
          timerId: c.state.id,
          durationMs: c.state.durationMs
        });
        c.broadcast("timerStarted", c.state);
        return c.state.durationMs;
      });
      await loopCtx.sleep("countdown", durationMs);
      await loopCtx.step("complete-timer", async (c) => {
        c.state.completedAt = Date.now();
        c.broadcast("timerCompleted", c.state);
        c.log.info({
          msg: "timer completed",
          timerId: c.state.id
        });
      });
      return Loop8.break(void 0);
    });
  }),
  options: {
    sleepTimeout: 1e3
  }
});

// src/actors/workflow/workflow-fixtures.ts
import { actor as actor61, event as event18, queue as queue7 } from "rivetkit";
import { Loop as Loop9, workflow as workflow9 } from "rivetkit/workflow";
var WORKFLOW_GUARD_KV_KEY = "__rivet_actor_workflow_guard_triggered";
var WORKFLOW_QUEUE_NAME = "workflow-default";
var WORKFLOW_TIMEOUT_QUEUE_NAME = "workflow-timeout";
var workflowCounterActor = actor61({
  state: {
    runCount: 0,
    guardTriggered: false,
    history: []
  },
  run: workflow9(async (ctx) => {
    let leakedCtx;
    await ctx.loop("counter", async (loopCtx) => {
      await loopCtx.step("increment", async (c) => {
        incrementWorkflowCounter(c);
        leakedCtx = c;
      });
      if (leakedCtx) {
        try {
          leakedCtx.state;
        } catch {
        }
      }
      await loopCtx.sleep("idle", 25);
      return Loop9.continue(void 0);
    });
  }),
  actions: {
    getState: async (c) => {
      const guardFlag = await c.kv.get(WORKFLOW_GUARD_KV_KEY);
      if (guardFlag === "true") {
        c.state.guardTriggered = true;
      }
      return c.state;
    }
  },
  options: {
    sleepTimeout: 50
  }
});
var workflowQueueActor = actor61({
  state: {
    received: []
  },
  queues: {
    [WORKFLOW_QUEUE_NAME]: queue7()
  },
  run: workflow9(async (ctx) => {
    await ctx.loop("queue", async (loopCtx) => {
      const message = await loopCtx.queue.next("queue-wait", {
        names: [WORKFLOW_QUEUE_NAME],
        completable: true
      });
      if (!message.complete) {
        return Loop9.continue(void 0);
      }
      const complete = message.complete;
      await loopCtx.step("store-message", async (c) => {
        await storeWorkflowQueueMessage(c, message.body, complete);
      });
      return Loop9.continue(void 0);
    });
  }),
  actions: {
    getMessages: (c) => c.state.received
  }
});
var workflowSleepActor = actor61({
  state: {
    ticks: 0
  },
  run: workflow9(async (ctx) => {
    await ctx.loop("sleep", async (loopCtx) => {
      await loopCtx.step("tick", async (c) => {
        incrementWorkflowSleepTick(c);
      });
      await loopCtx.sleep("delay", 40);
      return Loop9.continue(void 0);
    });
  }),
  actions: {
    getState: (c) => c.state
  },
  options: {
    sleepTimeout: 50
  }
});
var workflowQueueTimeoutActor = actor61({
  state: {
    processed: 0,
    ticks: 0,
    lastTickAt: null,
    lastJob: null,
    timeoutMs: 2e3
  },
  events: {
    tick: event18(),
    jobProcessed: event18()
  },
  queues: {
    [WORKFLOW_TIMEOUT_QUEUE_NAME]: queue7()
  },
  run: workflow9(async (ctx) => {
    await ctx.loop("queue-timeout-loop", async (loopCtx) => {
      const timeoutMs = await loopCtx.step(
        "read-timeout",
        async (c) => readWorkflowTimeoutMs(c)
      );
      const [message] = await loopCtx.queue.nextBatch(
        "wait-job-or-timeout",
        {
          names: [WORKFLOW_TIMEOUT_QUEUE_NAME],
          timeout: timeoutMs
        }
      );
      if (!message) {
        await loopCtx.step("tick", async (c) => {
          recordWorkflowTimeoutTick(c);
        });
        return Loop9.continue(void 0);
      }
      await loopCtx.step("process-job", async (c) => {
        processWorkflowTimeoutJob(c, message.body);
      });
      return Loop9.continue(void 0);
    });
  }),
  actions: {
    enqueueJob: async (c, payload2) => {
      const job = { id: crypto.randomUUID(), payload: payload2 };
      await c.queue.send(WORKFLOW_TIMEOUT_QUEUE_NAME, job);
      return job;
    },
    setTimeoutMs: (c, timeoutMs) => {
      c.state.timeoutMs = Math.max(100, Math.floor(timeoutMs));
      return c.state.timeoutMs;
    },
    getState: (c) => c.state
  }
});
function incrementWorkflowCounter(ctx) {
  ctx.state.runCount += 1;
  ctx.state.history.push(ctx.state.runCount);
}
async function storeWorkflowQueueMessage(ctx, body, complete) {
  ctx.state.received.push(body);
  await complete({ echo: body });
}
function incrementWorkflowSleepTick(ctx) {
  ctx.state.ticks += 1;
}
function readWorkflowTimeoutMs(ctx) {
  return ctx.state.timeoutMs;
}
function recordWorkflowTimeoutTick(ctx) {
  const at = Date.now();
  ctx.state.ticks += 1;
  ctx.state.lastTickAt = at;
  ctx.broadcast("tick", {
    ticks: ctx.state.ticks,
    at
  });
}
function processWorkflowTimeoutJob(ctx, job) {
  ctx.state.processed += 1;
  ctx.state.lastJob = job;
  ctx.broadcast("jobProcessed", {
    processed: ctx.state.processed,
    job
  });
}

// src/mode.ts
function resolveMode() {
  const explicit = process.env.RIVET_KITCHEN_SINK_MODE;
  if (explicit === "serverless" || explicit === "serverful" || explicit === "serverless-local") {
    return explicit;
  }
  if (explicit !== void 0 && explicit !== "") {
    throw new Error(
      `RIVET_KITCHEN_SINK_MODE must be one of "serverless", "serverful", or "serverless-local" (got "${explicit}")`
    );
  }
  if (process.env.RIVET_RUN_ENGINE === "1") return "serverless-local";
  if (process.env.RIVET_SERVERLESS_URL !== void 0)
    return "serverless-local";
  if (process.env.KITCHEN_SINK_SERVERLESS_URL !== void 0) {
    return "serverless-local";
  }
  return "serverless";
}

// src/index.ts
function numberFromEnv2(name, fallback) {
  const value = process.env[name];
  if (value === void 0 || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`${name} must be a finite number`);
  }
  return parsed;
}
function serverlessPoolConfig() {
  if (resolveMode() !== "serverless-local") return void 0;
  const url = process.env.RIVET_SERVERLESS_URL ?? process.env.KITCHEN_SINK_SERVERLESS_URL ?? "http://127.0.0.1:3000/api/rivet";
  return {
    name: process.env.RIVET_POOL,
    url,
    requestLifespan: numberFromEnv2(
      "RIVET_SERVERLESS_REQUEST_LIFESPAN",
      15 * 60
    ),
    drainGracePeriod: numberFromEnv2(
      "RIVET_SERVERLESS_DRAIN_GRACE_PERIOD",
      15 * 60
    ),
    metadataPollInterval: numberFromEnv2(
      "RIVET_SERVERLESS_METADATA_POLL_INTERVAL_MS",
      1e3
    ),
    metadata: {
      source: "kitchen-sink",
      smoke: "raw-websocket-serverless"
    }
  };
}
var registry2 = setup2({
  sqlite: process.env.RIVET_KITCHEN_SINK_REMOTE_SQLITE === "1" ? { backend: "remote" } : void 0,
  configurePool: serverlessPoolConfig(),
  serverless: {
    publicToken: process.env.RIVET_PUBLIC_TOKEN ?? process.env.RIVET_TOKEN ?? "dev",
    maxStartPayloadBytes: numberFromEnv2(
      "RIVET_SERVERLESS_MAX_START_PAYLOAD_BYTES",
      16 * 1024 * 1024
    )
  },
  use: {
    // Overview + state basics
    counter,
    counterConn,
    counterWithParams,
    counterWithLifecycle,
    pingPongCounter,
    // Core API
    inputActor,
    syncActionActor,
    asyncActionActor,
    promiseActor,
    shortTimeoutActor,
    longTimeoutActor,
    defaultTimeoutActor,
    syncTimeoutActor,
    customTimeoutActor,
    errorHandlingActor,
    // State and storage
    onStateChangeActor,
    metadataActor,
    staticVarActor,
    nestedVarActor,
    dynamicVarActor,
    uniqueVarActor,
    driverCtxActor,
    kvActor,
    largePayloadActor,
    largePayloadConnActor,
    sqliteRawActor,
    sqliteDrizzleActor,
    parallelismTest,
    // Realtime and connections
    connStateActor,
    rejectConnectionActor,
    requestAccessActor,
    // HTTP and WebSocket
    rawHttpActor,
    rawHttpNoHandlerActor,
    rawHttpVoidReturnActor,
    rawHttpHonoActor,
    rawHttpRequestPropertiesActor,
    rawWebSocketActor,
    rawWebSocketBinaryActor,
    rawFetchCounter,
    rawWebSocketChatRoom,
    rawWebSocketServerlessSmoke,
    tunnelStress,
    // Lifecycle and scheduling
    runWithTicks,
    runWithQueueConsumer,
    runWithEarlyExit,
    runWithError,
    runWithoutHandler,
    sleep: sleep2,
    sleepWithLongRpc,
    sleepWithNoSleepOption,
    sleepWithRawHttp,
    sleepWithRawWebSocket,
    scheduled,
    destroyActor,
    destroyObserver,
    hibernationActor,
    // Queues
    worker,
    workerTimeout,
    // Workflows
    timer,
    order,
    batch,
    approval,
    dashboard,
    race,
    payment,
    workflowHistorySimple,
    workflowHistoryLoop,
    workflowHistoryJoin,
    workflowHistoryRace,
    workflowHistoryFull,
    workflowHistoryInProgress,
    workflowHistoryRetrying,
    workflowHistoryFailed,
    workflowCounterActor,
    workflowQueueActor,
    workflowSleepActor,
    workflowQueueTimeoutActor,
    // Inter-actor
    inventory,
    checkout,
    // Testing fixtures
    inlineClientActor,
    testCounter,
    testCounterSqlite,
    testSqliteLoad,
    testSqliteBench,
    sqliteColdStartBench,
    sqliteRealworldBench,
    rawSqliteFuzzer,
    sqliteMemoryPressure,
    growDb,
    mockAgenticLoop,
    sleepCloseFuzz,
    loadTestAgent,
    loadTestAgent2,
    sigtermSleepProbe,
    slowReconnectActor,
    // AI
    aiAgent
  }
});

// src/server.ts
var app = new Hono3();
var port = Number.parseInt(process.env.PORT ?? "3000", 10);
var mode = resolveMode();
process.on("exit", (code) => {
  console.log(
    JSON.stringify({ kind: "process_exit", code, pid: process.pid })
  );
});
if (process.env.SQLITE_MEMORY_SOAK_DIAGNOSTICS === "1") {
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => {
      console.log(
        JSON.stringify({
          kind: "process_signal",
          signal,
          pid: process.pid,
          ppid: process.ppid,
          timestamp: (/* @__PURE__ */ new Date()).toISOString()
        })
      );
      process.exit(signal === "SIGINT" ? 130 : 143);
    });
  }
}
process.on("beforeExit", (code) => {
  console.log(
    JSON.stringify({ kind: "process_before_exit", code, pid: process.pid })
  );
});
process.on("uncaughtException", (error) => {
  console.error(
    JSON.stringify({
      kind: "uncaught_exception",
      error: error.stack ?? error.message
    })
  );
});
process.on("unhandledRejection", (reason) => {
  console.error(
    JSON.stringify({
      kind: "unhandled_rejection",
      error: reason instanceof Error ? reason.stack ?? reason.message : String(reason)
    })
  );
});
async function memoryBreakdown(forceGc) {
  const gc = globalThis.gc;
  if (forceGc && typeof gc === "function") gc();
  const memory = process.memoryUsage();
  const heap = v8.getHeapStatistics();
  const spaces = v8.getHeapSpaceStatistics();
  const nativeNonV8Estimate = Math.max(0, memory.rss - heap.total_heap_size);
  return {
    pid: process.pid,
    timestamp: (/* @__PURE__ */ new Date()).toISOString(),
    uptimeSeconds: process.uptime(),
    gcRequested: forceGc,
    gcAvailable: typeof gc === "function",
    process: {
      rssBytes: memory.rss,
      heapTotalBytes: memory.heapTotal,
      heapUsedBytes: memory.heapUsed,
      externalBytes: memory.external,
      arrayBuffersBytes: memory.arrayBuffers
    },
    v8: {
      totalHeapSizeBytes: heap.total_heap_size,
      usedHeapSizeBytes: heap.used_heap_size,
      heapSizeLimitBytes: heap.heap_size_limit,
      mallocedMemoryBytes: heap.malloced_memory,
      externalMemoryBytes: heap.external_memory,
      peakMallocedMemoryBytes: heap.peak_malloced_memory,
      spaces: spaces.map((space) => ({
        name: space.space_name,
        sizeBytes: space.space_size,
        usedBytes: space.space_used_size,
        availableBytes: space.space_available_size,
        physicalSizeBytes: space.physical_space_size
      }))
    },
    estimates: {
      jsHeapResidentBytes: memory.heapTotal,
      jsHeapUsedBytes: memory.heapUsed,
      v8ExternalBytes: memory.external,
      nativeNonV8ResidentEstimateBytes: nativeNonV8Estimate
    },
    resourceUsage: process.resourceUsage()
  };
}
app.get("/debug/memory", async (c) => {
  const forceGc = c.req.query("gc") === "1";
  return c.json(await memoryBreakdown(forceGc));
});
app.get("/health", (c) => c.json({ ok: true }));
app.get("/metadata", () => registry2.routes.metadata());
app.get("/metrics", (c) => registry2.routes.prometheusMetrics(c.req.raw));
app.post("/debug/heap-snapshot", (c) => {
  if (process.env.SQLITE_MEMORY_SOAK_DIAGNOSTICS !== "1") {
    return c.json({ error: "disabled" }, 404);
  }
  const path = c.req.query("path");
  if (!path) {
    return c.json({ error: "missing path" }, 400);
  }
  const writtenPath = v8.writeHeapSnapshot(path);
  return c.json({ path: writtenPath });
});
app.use("*", async (_c, next) => {
  const _startedAt = Date.now();
  await next();
});
if (mode === "serverful") {
  registry2.start();
} else {
  app.all("/api/rivet/*", (c) => registry2.handler(c.req.raw));
  app.all("/api/rivet", (c) => registry2.handler(c.req.raw));
}
var distDir = "dist";
var indexPath = resolve(process.cwd(), distDir, "index.html");
if (existsSync(indexPath)) {
  app.use("/*", serveStatic({ root: distDir }));
  const indexHtml = readFileSync(indexPath, "utf8");
  app.get("/*", (c) => {
    const path = new URL(c.req.url).pathname;
    const last = path.slice(path.lastIndexOf("/") + 1);
    if (last.includes(".")) return c.notFound();
    return c.html(indexHtml);
  });
}
var server = serve({ fetch: app.fetch, port }, () => {
  if (mode === "serverful") {
    console.log(
      `kitchen sink (serverful) listening on http://127.0.0.1:${port}`
    );
  } else {
    console.log(
      `kitchen sink (${mode}) listening on http://127.0.0.1:${port}/api/rivet`
    );
  }
});
var httpServer = server;
httpServer.requestTimeout = 0;
httpServer.headersTimeout = 0;
httpServer.keepAliveTimeout = 0;
httpServer.timeout = 0;
//# sourceMappingURL=server.mjs.map