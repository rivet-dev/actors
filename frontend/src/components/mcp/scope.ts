export type Scope = "namespace" | "project" | "organization" | "account";

export interface HostedTarget {
	organization: string;
	project: string;
	namespace: string;
}

export const SCOPES: Record<
	Scope,
	{ label: string; reach: string; pins: (keyof HostedTarget)[] }
> = {
	namespace: {
		label: "This namespace",
		reach: "only this namespace",
		pins: ["organization", "project", "namespace"],
	},
	project: {
		label: "This project",
		reach: "any namespace in this project",
		pins: ["organization", "project"],
	},
	organization: {
		label: "This organization",
		reach: "any project in this organization",
		pins: ["organization"],
	},
	account: {
		label: "Entire account",
		reach: "any project in your account",
		pins: [],
	},
};

export const SCOPE_ORDER: Scope[] = [
	"namespace",
	"project",
	"organization",
	"account",
];

// Levels left out of the query stay open for the agent to name per call. Every
// level that is present is a hard pin the session cannot move off, so a
// narrower scope is the safer default.
export function hostedUrl(
	base: string,
	target: HostedTarget,
	scope: Scope,
): string {
	const query = new URLSearchParams();
	for (const level of SCOPES[scope].pins) query.set(level, target[level]);
	const search = query.toString();
	return search ? `${base}?${search}` : base;
}
