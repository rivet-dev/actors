#!/usr/bin/env bash
# API test suite for the webhook endpoints.
#
# Covers everything in the webhook spec that is reachable over HTTP: config CRUD, the event-type
# allowlist, destination/header validation, event history, and the retry endpoint's error paths.
#
# Does NOT cover actual webhook delivery. Nothing in the API can fire a `Trigger` - the only
# producer is the runner pool error tracker - so delivery, retry-of-a-real-delivery, and event
# recording need a runner pool error to exercise. See the webhook spec.
#
# Usage:
#   scripts/run/engine-rocksdb.sh          # in another shell
#   scripts/tests/webhooks_api.sh
#
# Env:
#   RIVET_ENDPOINT  default http://localhost:6420
#   RIVET_TOKEN     bearer token; omit when the engine runs without auth configured
#   NAMESPACE       default webhook-test

set -uo pipefail

R="${RIVET_ENDPOINT:-http://localhost:6420}"
NS="${NAMESPACE:-webhook-test}"
AUTH=()
if [ -n "${RIVET_TOKEN:-}" ]; then
	AUTH=(-H "Authorization: Bearer ${RIVET_TOKEN}")
fi

pass=0
fail=0
RESP=""
STATUS=""

# Extracts "group.code" from an error body, or "ok" for a success body.
errcode() {
	python3 - "$1" <<'PY'
import sys, json
try:
    d = json.loads(sys.argv[1])
except Exception:
    print("<not-json>"); sys.exit()
if isinstance(d, dict) and "group" in d and "code" in d:
    print(str(d["group"]) + "." + str(d["code"]))
else:
    print("ok")
PY
}

# Reads a dotted path out of a JSON body, printing <missing> when absent.
jpath() {
	python3 - "$1" "$2" <<'PY'
import sys, json
try:
    cur = json.loads(sys.argv[1])
except Exception:
    print("<not-json>"); sys.exit()
for part in sys.argv[2].split("."):
    if isinstance(cur, dict) and part in cur:
        cur = cur[part]
    elif isinstance(cur, list) and part.isdigit() and int(part) < len(cur):
        cur = cur[int(part)]
    else:
        print("<missing>"); sys.exit()
print("null" if cur is None else (json.dumps(cur, sort_keys=True, separators=(",", ":")) if isinstance(cur, (dict, list)) else cur))
PY
}

req() { # req METHOD PATH [BODY]
	local method="$1" path="$2" body="${3:-}"
	local out
	if [ -n "$body" ]; then
		out=$(curl -s -w '\n%{http_code}' -X "$method" "$R$path" ${AUTH[@]+"${AUTH[@]}"} \
			-H 'Content-Type: application/json' -d "$body")
	else
		out=$(curl -s -w '\n%{http_code}' -X "$method" "$R$path" ${AUTH[@]+"${AUTH[@]}"})
	fi
	STATUS="${out##*$'\n'}"
	RESP="${out%$'\n'*}"
}

ok() { pass=$((pass + 1)); printf '  \033[32mPASS\033[0m %s\n' "$1"; }
no() {
	fail=$((fail + 1))
	printf '  \033[31mFAIL\033[0m %s\n        expected: %s\n        got:      %s\n' "$1" "$2" "$3"
}

expect_err() { # expect_err NAME EXPECTED
	local got; got=$(errcode "$RESP")
	[ "$got" = "$2" ] && ok "$1" || no "$1" "$2" "$got  (body: $RESP)"
}

expect_field() { # expect_field NAME PATH EXPECTED
	local got; got=$(jpath "$RESP" "$2")
	[ "$got" = "$3" ] && ok "$1" || no "$1" "$3" "$got"
}

section() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# ---------------------------------------------------------------- setup

if ! curl -sf -o /dev/null --max-time 3 "$R/health"; then
	echo "engine not reachable at $R - start it with scripts/run/engine-rocksdb.sh" >&2
	exit 1
fi

req POST /namespaces "{\"name\":\"$NS\",\"display_name\":\"Webhook Test\"}"
case "$(errcode "$RESP")" in
	ok|namespace.name_not_unique) ;;
	*) echo "could not create or reuse namespace $NS: $RESP" >&2; exit 1 ;;
esac

# Start from a known state.
req DELETE "/webhooks/wh-main?namespace=$NS" >/dev/null 2>&1

# ---------------------------------------------------------------- tests

section "config CRUD"
req PUT "/webhooks/wh-main?namespace=$NS" \
	'{"url":"http://127.0.0.1:8099/v1","headers":{"X-Token":"abc"},"subscriptions":["runner_pool.error"]}'
expect_err   "create webhook" ok

req GET "/webhooks?namespace=$NS"
expect_field "create is listed"        "webhooks.wh-main.url" "http://127.0.0.1:8099/v1"
expect_field "create stored headers"   "webhooks.wh-main.headers.X-Token" "abc"
expect_field "create stored subs"      "webhooks.wh-main.subscriptions" '["runner_pool.error"]'

# Regression: update used to fail because epoxy v2 rejects a value-based CheckAndSet expectation.
req PUT "/webhooks/wh-main?namespace=$NS" \
	'{"url":"http://127.0.0.1:8099/v2","subscriptions":["runner_pool.error","runner_pool.healthy"]}'
expect_err   "update existing webhook" ok

req GET "/webhooks?namespace=$NS"
expect_field "update changed url"      "webhooks.wh-main.url" "http://127.0.0.1:8099/v2"
expect_field "update changed subs"     "webhooks.wh-main.subscriptions" '["runner_pool.error","runner_pool.healthy"]'
expect_field "update cleared headers"  "webhooks.wh-main.headers" '{}'

section "event history"
req GET "/webhooks/wh-main/events?namespace=$NS"
expect_err   "events on real webhook"       ok
expect_field "events empty history"         "events" "[]"
expect_field "events null cursor"           "pagination.cursor" "null"

req GET "/webhooks/wh-main/events?namespace=$NS&limit=2"
expect_err   "events honours limit param"   ok

req GET "/webhooks/nope/events?namespace=$NS"
expect_err   "events on missing webhook"    webhook.not_found

req GET "/webhooks/wh-main/events?namespace=$NS&cursor=garbage"
expect_err   "events rejects bad cursor"    api.bad_request

req GET "/webhooks/wh-main/events?namespace=$NS&cursor=notanum:abc"
expect_err   "events rejects non-int cursor" api.bad_request

req GET "/webhooks/wh-main/events?namespace=$NS&cursor=123:abc"
expect_err   "events accepts valid cursor"  ok

section "retry endpoint"
req POST "/webhooks/wh-main/deliveries/00000000-0000-0000-0000-000000000000/retry?namespace=$NS"
expect_err   "retry unknown delivery"       webhook.delivery_not_found

req POST "/webhooks/nope/deliveries/abc/retry?namespace=$NS"
expect_err   "retry on missing webhook"     webhook.delivery_not_found

section "event-type allowlist"
req PUT "/webhooks/wh-bad?namespace=$NS" \
	'{"url":"http://127.0.0.1:8099/x","subscriptions":["actor.http_request"]}'
expect_err   "rejects high-throughput type" api.bad_request

req PUT "/webhooks/wh-bad?namespace=$NS" \
	'{"url":"http://127.0.0.1:8099/x","subscriptions":["nonsense"]}'
expect_err   "rejects unknown event type"   api.bad_request

section "destination validation (SSRF)"
req PUT "/webhooks/wh-bad?namespace=$NS" '{"url":"not-a-url","subscriptions":[]}'
expect_err   "rejects malformed url"        webhook.invalid

req PUT "/webhooks/wh-bad?namespace=$NS" '{"url":"http://169.254.169.254/latest/meta-data/","subscriptions":[]}'
expect_err   "rejects link-local metadata"  webhook.invalid

req PUT "/webhooks/wh-bad?namespace=$NS" '{"url":"http://10.0.0.1/hook","subscriptions":[]}'
expect_err   "rejects private network"      webhook.invalid

section "header validation"
req PUT "/webhooks/wh-bad?namespace=$NS" \
	"$(python3 -c 'import json;print(json.dumps({"url":"http://127.0.0.1:8099/x","headers":{f"X-{i}":"v" for i in range(17)},"subscriptions":[]}))')"
expect_err   "rejects >16 headers"          webhook.invalid

req PUT "/webhooks/wh-bad?namespace=$NS" \
	"$(python3 -c 'import json;print(json.dumps({"url":"http://127.0.0.1:8099/x","headers":{"X-Big":"a"*5000},"subscriptions":[]}))')"
expect_err   "rejects oversize header value" webhook.invalid

req PUT "/webhooks/wh-bad?namespace=$NS" '{"url":"http://127.0.0.1:8099/x","headers":{"Bad Header":"v"},"subscriptions":[]}'
expect_err   "rejects invalid header name"  webhook.invalid

req PUT "/webhooks/wh-bad?namespace=$NS" '{"url":"http://127.0.0.1:8099/x","bogus":1}'
expect_err   "rejects unknown body field"   api.bad_request

section "failed update leaves config intact"
req PUT "/webhooks/wh-main?namespace=$NS" '{"url":"nope","subscriptions":[]}'
expect_err   "bad update is rejected"       webhook.invalid
req GET "/webhooks?namespace=$NS"
expect_field "config unchanged after fail"  "webhooks.wh-main.url" "http://127.0.0.1:8099/v2"

section "namespace validation"
req GET  "/webhooks?namespace=no-such-ns";                              expect_err "list bad namespace"   namespace.not_found
req GET  "/webhooks/wh-main/events?namespace=no-such-ns";               expect_err "events bad namespace" namespace.not_found
req PUT  "/webhooks/wh-main?namespace=no-such-ns" '{"url":"http://127.0.0.1:8099/x","subscriptions":[]}'
expect_err "upsert bad namespace" namespace.not_found
req DELETE "/webhooks/wh-main?namespace=no-such-ns";                    expect_err "delete bad namespace" namespace.not_found
req POST "/webhooks/wh-main/deliveries/abc/retry?namespace=no-such-ns"; expect_err "retry bad namespace"  namespace.not_found

section "delete"
req DELETE "/webhooks/wh-main?namespace=$NS"
expect_err   "delete existing webhook"      ok
req DELETE "/webhooks/wh-main?namespace=$NS"
expect_err   "delete is idempotent"         ok
req GET "/webhooks?namespace=$NS"
expect_field "webhook is gone"              "webhooks" '{}'

# ---------------------------------------------------------------- summary

printf '\n\033[1m%d passed, %d failed\033[0m\n' "$pass" "$fail"
printf 'not covered: webhook delivery, retry of a real delivery, and event recording.\n'
printf 'those need a runner pool error to fire a Trigger; no API endpoint can produce one.\n'
[ "$fail" -eq 0 ]
