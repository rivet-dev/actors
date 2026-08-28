import { describe, expect, it } from "vitest";
import { hostedUrl, type Scope } from "./mcp-scope";

const BASE = "https://mcp.rivet.dev/mcp";
const TARGET = {
	organization: "acme",
	project: "prod",
	namespace: "canary",
};

function params(scope: Scope) {
	return Object.fromEntries(
		new URL(hostedUrl(BASE, TARGET, scope)).searchParams,
	);
}

describe("hostedUrl", () => {
	it("pins every level for the namespace scope", () => {
		expect(params("namespace")).toEqual(TARGET);
	});

	it("leaves the namespace open for the project scope", () => {
		expect(params("project")).toEqual({
			organization: "acme",
			project: "prod",
		});
	});

	it("leaves the project open for the organization scope", () => {
		expect(params("organization")).toEqual({ organization: "acme" });
	});

	it("emits no query at all for the account scope", () => {
		expect(hostedUrl(BASE, TARGET, "account")).toBe(BASE);
	});
});
