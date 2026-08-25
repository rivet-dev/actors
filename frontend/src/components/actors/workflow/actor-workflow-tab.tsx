import { faSpinnerThird, Icon } from "@rivet-gg/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type PropsWithChildren, useMemo } from "react";
import { Button, toast } from "@/components";
import {
	actorInspectorQueriesKeys,
	useActorInspector,
} from "../actor-inspector-context";
import { useDataProvider } from "../data-provider";
import type { ActorId } from "../queries";
import type { HistoryItem, WorkflowHistory } from "./workflow-types";
import { WorkflowVisualizer } from "./workflow-visualizer";

interface ActorWorkflowTabProps {
	actorId: ActorId;
}

export function ActorWorkflowTab({ actorId }: ActorWorkflowTabProps) {
	const inspector = useActorInspector();
	const queryClient = useQueryClient();
	const dataProvider = useDataProvider();

	const { data: isWorkflowEnabled, isLoading: isEnabledLoading } = useQuery(
		inspector.actorIsWorkflowEnabledQueryOptions(actorId),
	);
	const { data: actorStatus } = useQuery(
		dataProvider.actorStatusQueryOptions(actorId),
	);

	const { data: workflowData, isLoading: isHistoryLoading } = useQuery(
		inspector.actorWorkflowHistoryQueryOptions(actorId),
	);

	const isLoading = isEnabledLoading || isHistoryLoading;
	const workflow = workflowData?.history ?? null;
	const historyError = workflowData?.error ?? null;
	const currentStep = useMemo(() => getCurrentStep(workflow), [workflow]);
	const hasHiddenRunningStep =
		actorStatus === "running" &&
		workflow?.history.length !== 0 &&
		workflow?.history.every((item) => item.entry.status === "completed");
	const replayMutation = useMutation(
		inspector.actorWorkflowReplayMutation(actorId),
	);
	const canReplayCurrentStep =
		inspector.inspectorProtocolVersion >= 4 &&
		currentStep?.entry.status !== "running" &&
		currentStep?.entry.retryCount !== undefined &&
		currentStep.entry.retryCount > 0;
	const replayingEntryId = replayMutation.isPending
		? replayMutation.variables
		: undefined;

	const handleReplay = async (entryId?: string) => {
		try {
			const result = await replayMutation.mutateAsync(entryId);
			// History itself is refreshed over the websocket below; the replay
			// response does not carry it in a form this tab can render.
			queryClient.setQueryData(
				actorInspectorQueriesKeys.actorIsWorkflowEnabled(actorId),
				result.isEnabled,
			);
			void syncWorkflowHistoryAfterReplay({
				actorId,
				inspector,
				queryClient,
			});
			toast.success(
				entryId
					? "Workflow replay scheduled from selected step."
					: "Workflow replay scheduled from the beginning.",
			);
		} catch (error) {
			toast.error(
				error instanceof Error
					? error.message
					: "Failed to replay workflow step.",
			);
		}
	};

	if (isLoading) {
		return (
			<Info>
				<div className="flex items-center">
					<Icon icon={faSpinnerThird} className="animate-spin mr-2" />
					Loading Workflow...
				</div>
			</Info>
		);
	}

	if (!isWorkflowEnabled) {
		return (
			<Info>
				<p>
					Workflow Visualizer is not enabled for this Actor. <br />{" "}
					This feature requires a workflow-based Actor.
				</p>
			</Info>
		);
	}

	if (historyError) {
		return (
			<Info>
				<p>
					Workflow history could not be read from this Actor. <br />
					{historyError}
				</p>
			</Info>
		);
	}

	if (!workflow) {
		return (
			<Info>
				<p>No workflow history available yet.</p>
			</Info>
		);
	}

	return (
		<div className="flex-1 w-full min-h-0 h-full flex flex-col">
			<div className="flex items-center justify-between gap-4 border-b border-border px-4 py-3 text-sm">
				<p className="text-muted-foreground">
					Right-click a previous step to replay the workflow from
					there.
				</p>
				{canReplayCurrentStep && currentStep && (
					<Button
						size="sm"
						variant="outline"
						disabled={replayMutation.isPending}
						onClick={() => handleReplay(currentStep.entry.id)}
					>
						Replay From Current Step
					</Button>
				)}
			</div>
			<WorkflowVisualizer
				workflow={workflow}
				currentStepId={currentStep?.entry.id}
				isReplayBlocked={hasHiddenRunningStep}
				replayingEntryId={replayingEntryId}
				onReplayStep={
					inspector.inspectorProtocolVersion >= 4
						? (entryId) => {
								void handleReplay(entryId);
							}
						: undefined
				}
			/>
		</div>
	);
}

function Info({ children }: PropsWithChildren) {
	return (
		<div className="flex-1 flex flex-col gap-2 items-center justify-center h-full text-center max-w-lg mx-auto">
			{children}
		</div>
	);
}

function getCurrentStep(workflow: WorkflowHistory | null): HistoryItem | null {
	if (!workflow) {
		return null;
	}

	const steps = workflow.history.filter(
		(item) => item.entry.kind.type === "step",
	);
	if (steps.length === 0) {
		return null;
	}

	const activeOrFailedSteps = steps.filter(
		(item) => item.entry.status !== "completed",
	);
	if (activeOrFailedSteps.length > 0) {
		return activeOrFailedSteps[activeOrFailedSteps.length - 1]!;
	}

	return steps[steps.length - 1]!;
}

async function syncWorkflowHistoryAfterReplay({
	actorId,
	inspector,
	queryClient,
}: {
	actorId: ActorId;
	inspector: ReturnType<typeof useActorInspector>;
	queryClient: ReturnType<typeof useQueryClient>;
}) {
	const workflowQueryKey =
		actorInspectorQueriesKeys.actorWorkflowHistory(actorId);
	const stateQueryKey = actorInspectorQueriesKeys.actorState(actorId);

	for (const delayMs of [250, 1_000, 2_500, 5_000]) {
		await new Promise((resolve) => setTimeout(resolve, delayMs));
		await queryClient.invalidateQueries({
			queryKey: workflowQueryKey,
			exact: true,
		});
		const result = await queryClient.fetchQuery({
			...inspector.actorWorkflowHistoryQueryOptions(actorId),
			staleTime: 0,
		});
		queryClient.setQueryData(
			actorInspectorQueriesKeys.actorIsWorkflowEnabled(actorId),
			result.isEnabled,
		);
		await queryClient.invalidateQueries({
			queryKey: stateQueryKey,
			exact: true,
		});
		const stateResult = await queryClient.fetchQuery({
			...inspector.actorStateQueryOptions(actorId),
			staleTime: 0,
		});
		queryClient.setQueryData(
			actorInspectorQueriesKeys.actorIsStateEnabled(actorId),
			stateResult.isEnabled,
		);
	}
}
