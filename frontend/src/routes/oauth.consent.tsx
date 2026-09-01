import { useMutation, useQuery } from "@tanstack/react-query";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { useMemo } from "react";
import { type Control, Controller, useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { Logo } from "@/app/logo";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Skeleton } from "@/components/ui/skeleton";
import { authClient } from "@/lib/auth";

const searchSchema = z.object({
	client_id: z.string(),
	scope: z.string(),
	oauth_query: z.string().optional(),
});

interface ScopeDetail {
	title: string;
	description: string;
}

// The MCP entrypoint verifies every request against rivet:cloud:read, so a
// grant without it yields a token that cannot call anything.
const REQUIRED_SCOPES = new Set(["rivet:cloud:read"]);

const SCOPE_DETAILS: Record<string, ScopeDetail> = {
	openid: {
		title: "Confirm your identity",
		description:
			"Share your Rivet user ID so the client knows who signed in.",
	},
	offline_access: {
		title: "Stay signed in",
		description:
			"Reconnect without asking you again. Access tokens expire after 15 minutes.",
	},
	"rivet:cloud:read": {
		title: "View your organizations and projects",
		description:
			"List organizations, projects, and namespaces, and read their usage metrics.",
	},
	"rivet:cloud:write": {
		title: "Manage your organizations and projects",
		description:
			"Change cloud resources on your behalf. Destructive and credential operations stay unavailable.",
	},
	"rivet:actors:read": {
		title: "View your actors",
		description:
			"List actors in the selected namespace and send them read-only requests.",
	},
	"rivet:actors:write": {
		title: "Modify your actors",
		description: "Send requests that change actor state or lifecycle.",
	},
	"rivet:inspector:read": {
		title: "Inspect actor internals",
		description:
			"Read actor state, database contents, and logs through the Actor Inspector.",
	},
	"rivet:inspector:write": {
		title: "Modify actor internals",
		description:
			"Edit state and run commands against an actor through the Actor Inspector.",
	},
};

export const Route = createFileRoute("/oauth/consent")({
	validateSearch: searchSchema,
	beforeLoad: async ({ location }) => {
		const session = await authClient.getSession();
		if (!session.data) {
			throw redirect({
				to: "/login",
				search: { from: `${location.pathname}${location.searchStr}` },
			});
		}
	},
	component: OAuthConsent,
});

interface ConsentFormValues {
	scopes: Record<string, boolean>;
}

// Both of these track the checkbox values, so they subscribe on their own
// rather than re-rendering the whole consent page on every toggle.
function WriteAccessNote({ control }: { control: Control<ConsentFormValues> }) {
	const granted = useWatch({ control, name: "scopes" });
	if (
		!granted["rivet:cloud:write"] &&
		!granted["rivet:actors:write"] &&
		!granted["rivet:inspector:write"]
	) {
		return null;
	}
	return (
		<p className="mt-3 text-xs text-muted-foreground">
			Write access also requires the MCP service's write policy to be
			enabled, so approving it here does not by itself allow changes.
		</p>
	);
}

function AuthorizeButton({
	control,
	requestedScopes,
	disabled,
	onClick,
}: {
	control: Control<ConsentFormValues>;
	requestedScopes: string[];
	disabled: boolean;
	onClick: () => void;
}) {
	const granted = useWatch({ control, name: "scopes" });
	const grantedCount = requestedScopes.filter(
		(scope) => granted[scope],
	).length;
	return (
		<Button disabled={disabled || grantedCount === 0} onClick={onClick}>
			Authorize
		</Button>
	);
}

function OAuthConsent() {
	const search = Route.useSearch();
	const requestedScopes = useMemo(
		() => search.scope.split(/\s+/).filter(Boolean),
		[search.scope],
	);
	const { control, handleSubmit } = useForm<ConsentFormValues>({
		defaultValues: {
			scopes: Object.fromEntries(
				requestedScopes.map((scope) => [scope, true]),
			),
		},
	});
	// Dynamically registered clients pick their own opaque client_id, so the
	// name they registered under is the only human-readable identifier.
	const { data: client, isPending: isClientPending } = useQuery({
		queryKey: ["oauth", "public-client", search.client_id],
		queryFn: async () => {
			const result = await authClient.oauth2.publicClient({
				query: { client_id: search.client_id },
			});
			if (result.error) {
				throw new Error(
					result.error.message ?? "Could not load client.",
				);
			}
			return result.data;
		},
		retry: false,
	});

	const consent = useMutation({
		mutationFn: async ({
			accept,
			values,
		}: {
			accept: boolean;
			values: ConsentFormValues;
		}) => {
			const result = await authClient.oauth2.consent({
				accept,
				// Only ever a subset of the originally requested scopes; the
				// provider rejects anything that was not asked for.
				scope: requestedScopes
					.filter((scope) => values.scopes[scope])
					.join(" "),
				// The provider verifies a signature over the full authorize
				// query. validateSearch drops the params it does not declare,
				// so the router's searchStr would fail that check.
				oauth_query:
					search.oauth_query ??
					window.location.search.replace(/^\?/, ""),
			});
			if (result.error || !result.data?.url) {
				throw new Error(
					result.error?.message ??
						"Could not complete OAuth consent.",
				);
			}
			return result.data.url;
		},
		onSuccess: (url) => window.location.assign(url),
	});

	const submit = (accept: boolean) =>
		handleSubmit((values) => consent.mutate({ accept, values }));

	return (
		<main className="flex min-h-screen items-center justify-center bg-background p-6">
			<section className="w-full max-w-lg rounded-xl border bg-card p-6 shadow-sm">
				<Logo className="mb-6 h-9" />

				<div className="flex items-start gap-3">
					{client?.logo_uri ? (
						<img
							src={client.logo_uri}
							alt=""
							className="size-10 shrink-0 rounded-lg border bg-muted object-contain"
						/>
					) : null}
					<div className="min-w-0">
						<h1 className="text-xl font-semibold">
							Authorize MCP access
						</h1>
						{isClientPending ? (
							<Skeleton className="mt-2 h-5 w-64" />
						) : (
							<p className="mt-2 text-sm text-muted-foreground">
								{client?.client_name ? (
									<>
										<span className="font-medium text-foreground">
											{client.client_name}
										</span>{" "}
										is requesting access to your Rivet
										account.
									</>
								) : (
									"An application is requesting access to your Rivet account."
								)}
							</p>
						)}
					</div>
				</div>

				<h2 className="mt-6 text-xs font-medium uppercase tracking-wide text-muted-foreground">
					Choose what to allow
				</h2>
				<ul className="mt-3 divide-y rounded-lg border">
					{requestedScopes.map((scope) => {
						const detail = SCOPE_DETAILS[scope];
						const required = REQUIRED_SCOPES.has(scope);
						const id = `scope-${scope}`;
						return (
							<li key={scope} className="flex gap-3 px-4 py-3">
								<Controller
									control={control}
									name={`scopes.${scope}`}
									render={({ field }) => (
										<Checkbox
											id={id}
											checked={field.value === true}
											disabled={required}
											onBlur={field.onBlur}
											onCheckedChange={(checked) =>
												field.onChange(checked === true)
											}
											className="mt-0.5"
										/>
									)}
								/>
								<div className="min-w-0">
									<label
										htmlFor={id}
										className="flex flex-wrap items-baseline gap-x-2 gap-y-1"
									>
										<span className="text-sm font-medium">
											{detail?.title ?? scope}
										</span>
										{detail ? (
											<code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] leading-none text-muted-foreground">
												{scope}
											</code>
										) : null}
										{required ? (
											<span className="text-[11px] text-muted-foreground">
												Required
											</span>
										) : null}
									</label>
									<p className="mt-1 text-sm text-muted-foreground">
										{detail?.description ??
											"Grants the client additional access to your Rivet account."}
									</p>
								</div>
							</li>
						);
					})}
				</ul>

				<WriteAccessNote control={control} />

				{consent.error ? (
					<p className="mt-4 text-sm text-destructive">
						{consent.error.message}
					</p>
				) : null}

				<div className="mt-6 flex items-center justify-end gap-2">
					<Button
						variant="outline"
						disabled={consent.isPending}
						onClick={submit(false)}
					>
						Deny
					</Button>
					<AuthorizeButton
						control={control}
						requestedScopes={requestedScopes}
						disabled={consent.isPending}
						onClick={submit(true)}
					/>
				</div>

				<dl className="mt-6 border-t pt-4 text-xs text-muted-foreground">
					<div className="flex items-baseline gap-2">
						<dt className="shrink-0">Client ID</dt>
						<dd className="truncate font-mono">
							{search.client_id}
						</dd>
					</div>
					{client?.client_uri ? (
						<div className="mt-1 flex items-baseline gap-2">
							<dt className="shrink-0">Website</dt>
							<dd className="truncate">
								<a
									href={client.client_uri}
									target="_blank"
									rel="noreferrer noopener"
									className="underline underline-offset-2 hover:text-foreground"
								>
									{client.client_uri}
								</a>
							</dd>
						</div>
					) : null}
				</dl>
			</section>
		</main>
	);
}
