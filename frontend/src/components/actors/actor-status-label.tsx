import { useQuery } from "@tanstack/react-query";
import { formatISO } from "date-fns";
import { isObject } from "lodash";
import { formatValue } from "@/lib/format-value";
import { match, P } from "ts-pattern";
import type { RivetActorError } from "@/queries/types";
import { CodePreview } from "../code-preview/code-preview";
import { cn } from "../lib/utils";
import { RelativeTime } from "../relative-time";
import {
	Accordion,
	AccordionContent,
	AccordionItem,
	AccordionTrigger,
} from "../ui/accordion";
import { ScrollArea } from "../ui/scroll-area";
import { Code } from "../ui/typography";
import { useDataProvider } from "./data-provider";
import type { ActorId, ActorStatus } from "./queries";

export const ACTOR_STATUS_LABEL_MAP = {
	unknown: "Unknown",
	starting: "Starting",
	running: "Running",
	stopped: "Destroyed",
	crashed: "Crashed",
	sleeping: "Sleeping",
	pending: "Pending",
	"crash-loop": "Crash Loop Backoff",
} satisfies Record<ActorStatus, string>;

export const ActorStatusLabel = ({ status }: { status?: ActorStatus }) => {
	return (
		<span className="whitespace-nowrap">
			{status ? ACTOR_STATUS_LABEL_MAP[status] : "Unknown"}
		</span>
	);
};

export const QueriedActorStatusLabel = ({
	actorId,
	showAdditionalInfo,
}: {
	actorId: ActorId;
	showAdditionalInfo?: boolean;
}) => {
	const { data: status = "unknown", isError } = useQuery(
		useDataProvider().actorStatusQueryOptions(actorId),
	);
	return (
		<>
			<ActorStatusLabel status={isError ? "unknown" : status} />
			{showAdditionalInfo && (
				<QueriedActorStatusAdditionalInfo actorId={actorId} />
			)}
		</>
	);
};

export function QueriedActorStatusAdditionalInfo({
	actorId,
}: {
	actorId: ActorId;
}) {
	const { data: { rescheduleTs, error } = {} } = useQuery(
		useDataProvider().actorStatusAdditionalInfoQueryOptions(actorId),
	);

	if (rescheduleTs) {
		return (
			<span>
				Will try to start again{" "}
				<span>
					<RelativeTime time={new Date(rescheduleTs)} /> (
					{formatISO(rescheduleTs)}){" "}
				</span>
			</span>
		);
	}

	if (error) {
		return <ActorError error={error} />;
	}

	return null;
}

export function ActorError({ error }: { error: object | string }) {
	return match(error)
		.with(P.string, (errMsg) =>
			match(errMsg)
				.with("no_capacity", () => (
					<p>No capacity available to start Actor.</p>
				))
				.with("internal_error", () => (
					<p>Actor has an internal error.</p>
				))
				.otherwise(() => <p>Unknown error: {errMsg}</p>),
		)
		.with(
			P.shape({
				runner_pool_error: P.shape({ runner_id: P.string })
					.or(P.shape({ serverless_http_error: P.any }))
					.or(P.string)
					.or(P.shape({ serverless_connection_error: P.any }))
					.or(P.shape({ serverless_destination_blocked: P.any }))
					.or(P.shape({ serverless_invalid_sse_payload: P.any })),
			}),
			(err) => <RunnerPoolError error={err.runner_pool_error} />,
		)
		.with(
			P.shape({ runner_no_response: P.shape({ runner_id: P.string }) }),
			(err) => (
				<p>
					Runner ({err.runner_no_response.runner_id}) was allocated
					but Actor did not respond.
				</p>
			),
		)
		.with(
			P.shape({
				runner_connection_lost: P.shape({ runner_id: P.string }),
			}),
			(err) => (
				<p>
					Runner ({err.runner_connection_lost.runner_id}) connection
					was lost (no recent ping, network issue, or crash).
				</p>
			),
		)
		.with(
			P.shape({
				runner_draining_timeout: P.shape({ runner_id: P.string }),
			}),
			(err) => (
				<p>
					Runner ({err.runner_draining_timeout.runner_id}) was
					draining but Actor didn't stop in time.
				</p>
			),
		)
		.with(
			P.shape({
				envoy_connection_lost: P.shape({ envoy_key: P.string }),
			}),
			(_err) => <p>Connection to Runner was lost.</p>,
		)
		.with(P.shape({ crashed: P.shape({ message: P.string }) }), (err) => (
			<p>Actor crashed. {err.crashed.message} </p>
		))
		.otherwise(() => {
			return <p>Unknown error.</p>;
		});
}

export function QueriedActorError({ actorId }: { actorId: ActorId }) {
	const { data: error, isError } = useQuery(
		useDataProvider().actorErrorQueryOptions(actorId),
	);

	if (isError || !error) {
		return null;
	}

	return <ActorError error={error} />;
}

export function RunnerPoolError({ error }: { error: RivetActorError }) {
	return match(error)
		.with(P.nullish, () => null)
		.with(P.string, (errStr) =>
			match(errStr)
				.with("internal_error", () => (
					<p>Internal error occurred in runner pool</p>
				))
				.with("downgrade", () => (
					<p>
						Runner pool was downgraded to an unsupported version.
						Revert to the higher version.
					</p>
				))
				.with("serverless_stream_ended_early", () => (
					<p>
						Connection terminated before runner stopped. Ensure that
						the Rivet provider request lifespan is configured to
						match the max duration of your serverless provider.
					</p>
				))
				.otherwise(() => <p>Unknown runner pool error</p>),
		)
		.with(
			P.shape({
				serverless_http_error: P.shape({
					status_code: P.number,
					body: P.string,
				}),
			}),
			(errObj) => {
				const { status_code, body } = errObj.serverless_http_error;
				const code = status_code ?? "unknown";
				return (
					<>
						<p>Serverless HTTP error with status code {code}</p>
						{body ? <ErrorDetails error={body} /> : null}
					</>
				);
			},
		)
		.with(
			P.shape({
				serverless_connection_error: P.shape({ message: P.string }),
			}),
			(errObj) => {
				const message = errObj.serverless_connection_error?.message;
				return (
					<>
						<p>Unable to connect to serverless endpoint</p>
						{message ? (
							<ErrorDetailsContent error={message} />
						) : null}
					</>
				);
			},
		)
		.with(
			P.shape({
				serverless_destination_blocked: P.shape({ reason: P.string }),
			}),
			(errObj) => {
				const reason = errObj.serverless_destination_blocked?.reason;
				return (
					<>
						<p>
							Serverless endpoint URL is not an allowed
							destination
						</p>
						{reason ? <ErrorDetailsContent error={reason} /> : null}
					</>
				);
			},
		)
		.with(
			P.shape({
				serverless_invalid_sse_payload: P.shape({ message: P.string }),
			}),
			(errObj) => {
				const message = errObj.serverless_invalid_sse_payload?.message;
				return (
					<>
						<p>Request payload validation failed</p>
						{message ? (
							<ErrorDetailsContent error={message} />
						) : null}
					</>
				);
			},
		)
		.otherwise(() => {
			return <p>Unknown runner pool error.</p>;
		});
}

export function ErrorDetails({
	error,
	defaultOpen,
	className,
}: {
	error: unknown;
	defaultOpen?: boolean;
	className?: string;
}) {
	return (
		<Accordion
			type="single"
			collapsible
			defaultValue={defaultOpen ? "error-details" : undefined}
			className={cn("max-w-full min-w-0 pb-0", className)}
		>
			<AccordionItem value="error-details">
				<AccordionTrigger className="!flex-initial !w-auto mx-auto gap-1.5 !p-0 text-xs font-medium text-muted-foreground hover:text-foreground hover:no-underline [&>svg]:!size-3 [&>svg]:opacity-60">
					View error details
				</AccordionTrigger>
				<AccordionContent className="max-w-full min-w-0 pb-0">
					<ErrorDetailsContent error={error} />
				</AccordionContent>
			</AccordionItem>
		</Accordion>
	);
}

export function ErrorDetailsContent({ error }: { error: unknown }) {
	const json =
		typeof error === "string"
			? tryJsonParse(error)
			: isObject(error)
				? error
				: null;

	return json ? (
		<div className="not-prose my-4 rounded-lg border p-1 bg-background">
			<ScrollArea className="w-full">
				<CodePreview
					language="json"
					className="text-left"
					code={json ? formatValue(json, true) : String(error)}
				/>
			</ScrollArea>
		</div>
	) : (
		<Code className="block whitespace-pre-wrap text-left">
			{String(error)}
		</Code>
	);
}

const tryJsonParse = (str: string) => {
	try {
		return JSON.parse(str);
	} catch {
		return null;
	}
};
