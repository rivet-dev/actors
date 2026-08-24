import type { Story } from "@ladle/react";
import {
	Background,
	BackgroundVariant,
	Controls,
	type Edge,
	MiniMap,
	type Node,
	ReactFlow,
	ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import "../../../../.ladle/ladle.css";
import {
	failedWorkflow,
	inProgressWorkflow,
	joinWorkflow,
	loopWorkflow,
	raceWorkflow,
	retryWorkflow,
	sampleWorkflowHistory,
	simpleLinearWorkflow,
	tryWorkflow,
} from "./workflow-example-data";
import { workflowHistoryToXYFlow } from "./workflow-to-xyflow";
import {
	NODE_HEIGHT,
	NODE_WIDTH,
	TYPE_COLORS,
	type WorkflowNodeData,
	workflowNodeTypes,
} from "./xyflow-nodes";

// Wrapper that applies dark theme and renders a minimal ReactFlow canvas.
function StoryCanvas({ nodes, edges = [] }: { nodes: Node[]; edges?: Edge[] }) {
	return (
		<div
			className="bg-background"
			style={{ width: "100%", height: "100%" }}
		>
			<ReactFlowProvider>
				<ReactFlow
					nodes={nodes}
					edges={edges}
					nodeTypes={workflowNodeTypes}
					fitView
					panOnScroll
					selectionOnDrag
					panOnDrag={[1, 2]}
					edgesFocusable={false}
					proOptions={{ hideAttribution: true }}
					colorMode="dark"
				>
					<Background
						variant={BackgroundVariant.Dots}
						gap={20}
						size={1.5}
						color="hsl(var(--border))"
					/>
					<Controls />
					<MiniMap
						nodeColor={(node) => {
							const entryType = node.data?.entryType as
								| string
								| undefined;
							if (
								typeof entryType === "string" &&
								entryType in TYPE_COLORS
							) {
								return TYPE_COLORS[
									entryType as keyof typeof TYPE_COLORS
								].icon;
							}
							return "#3b82f6";
						}}
						nodeStrokeColor="transparent"
						maskColor="hsl(20 14.3% 4.1% / 0.7)"
					/>
				</ReactFlow>
			</ReactFlowProvider>
		</div>
	);
}

// Helper to create a single centered node.
function singleNode(
	data: WorkflowNodeData,
): Node<WorkflowNodeData, "workflow">[] {
	return [
		{
			id: "1",
			type: "workflow",
			position: { x: 0, y: 0 },
			measured: { width: NODE_WIDTH, height: NODE_HEIGHT },
			data,
		},
	];
}

// ─── Individual Node Types ───────────────────────────────────

export const StepCompleted: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "fetch-user",
			summary: "completed",
			entryType: "step",
			status: "completed",
			duration: 342,
		})}
	/>
);
StepCompleted.storyName = "Nodes / Step Completed";

export const StepRunning: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "process-payment",
			summary: "in progress",
			entryType: "step",
			status: "running",
		})}
	/>
);
StepRunning.storyName = "Nodes / Step Running";

export const StepFailed: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "send-email",
			summary: "error",
			entryType: "step",
			status: "failed",
			error: "SMTP timeout",
		})}
	/>
);
StepFailed.storyName = "Nodes / Step Failed";

export const StepHandled: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "charge-card",
			summary: "handled error",
			entryType: "step",
			status: "failed",
			handledFailure: true,
			error: "card declined",
		})}
	/>
);
StepHandled.storyName = "Nodes / Step Handled";

export const StepRetrying: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "call-api",
			summary: "retrying",
			entryType: "step",
			status: "retrying",
			retryCount: 3,
		})}
	/>
);
StepRetrying.storyName = "Nodes / Step Retrying";

export const LoopNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "retry-loop",
			summary: "5 iterations",
			entryType: "loop",
			status: "completed",
			duration: 1250,
		})}
	/>
);
LoopNode.storyName = "Nodes / Loop";

export const SleepNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "wait-30s",
			summary: "completed",
			entryType: "sleep",
			status: "completed",
			duration: 30000,
		})}
	/>
);
SleepNode.storyName = "Nodes / Sleep";

export const MessageNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "user-confirmed",
			summary: "received",
			entryType: "message",
			status: "completed",
			duration: 120,
		})}
	/>
);
MessageNode.storyName = "Nodes / Message";

export const RollbackCheckpoint: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "save-state",
			summary: "checkpoint",
			entryType: "rollback_checkpoint",
			status: "completed",
			duration: 5,
		})}
	/>
);
RollbackCheckpoint.storyName = "Nodes / Rollback Checkpoint";

export const JoinNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "parallel-tasks",
			summary: "3/3 done",
			entryType: "join",
			status: "completed",
			duration: 890,
		})}
	/>
);
JoinNode.storyName = "Nodes / Join";

export const RaceNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "fastest-provider",
			summary: "winner: aws",
			entryType: "race",
			status: "completed",
			duration: 210,
		})}
	/>
);
RaceNode.storyName = "Nodes / Race";

export const RemovedNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "old-step",
			summary: "step",
			entryType: "removed",
			status: "completed",
		})}
	/>
);
RemovedNode.storyName = "Nodes / Removed";

export const VersionCheckNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "gate",
			summary: "v2",
			entryType: "version_check",
			status: "completed",
		})}
	/>
);
VersionCheckNode.storyName = "Nodes / Version Check";

export const InputNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "Input",
			summary: "3 fields",
			entryType: "input",
			status: "completed",
		})}
	/>
);
InputNode.storyName = "Nodes / Input";

export const OutputNode: Story = () => (
	<StoryCanvas
		nodes={singleNode({
			label: "Output",
			summary: "success",
			entryType: "output",
			status: "completed",
		})}
	/>
);
OutputNode.storyName = "Nodes / Output";

// ─── All Node Types Grid ─────────────────────────────────────

export const AllNodeTypes: Story = () => {
	const gap = NODE_HEIGHT + 48;
	const types: { id: string; data: WorkflowNodeData }[] = [
		{
			id: "input",
			data: {
				label: "Input",
				summary: "3 fields",
				entryType: "input",
				status: "completed",
			},
		},
		{
			id: "step",
			data: {
				label: "fetch-data",
				summary: "completed",
				entryType: "step",
				status: "completed",
				duration: 120,
			},
		},
		{
			id: "loop",
			data: {
				label: "retry-loop",
				summary: "5 iterations",
				entryType: "loop",
				status: "completed",
				duration: 1500,
			},
		},
		{
			id: "sleep",
			data: {
				label: "wait-30s",
				summary: "completed",
				entryType: "sleep",
				status: "completed",
				duration: 30000,
			},
		},
		{
			id: "message",
			data: {
				label: "user-confirmed",
				summary: "received",
				entryType: "message",
				status: "completed",
				duration: 200,
			},
		},
		{
			id: "checkpoint",
			data: {
				label: "save-state",
				summary: "checkpoint",
				entryType: "rollback_checkpoint",
				status: "completed",
				duration: 3,
			},
		},
		{
			id: "join",
			data: {
				label: "parallel-tasks",
				summary: "3/3 done",
				entryType: "join",
				status: "completed",
				duration: 890,
			},
		},
		{
			id: "race",
			data: {
				label: "fastest-provider",
				summary: "winner: aws",
				entryType: "race",
				status: "completed",
				duration: 210,
			},
		},
		{
			id: "removed",
			data: {
				label: "old-step",
				summary: "step",
				entryType: "removed",
				status: "completed",
			},
		},
		{
			id: "output",
			data: {
				label: "Output",
				summary: "success",
				entryType: "output",
				status: "completed",
			},
		},
	];

	const colSize = 5;
	const colGap = NODE_WIDTH + 40;
	const nodes: Node<WorkflowNodeData, "workflow">[] = types.map((t, i) => ({
		id: t.id,
		type: "workflow" as const,
		position: {
			x: Math.floor(i / colSize) * colGap,
			y: (i % colSize) * gap,
		},
		measured: { width: NODE_WIDTH, height: NODE_HEIGHT },
		data: t.data,
	}));

	return <StoryCanvas nodes={nodes} />;
};
AllNodeTypes.storyName = "Nodes / All Types Grid";

// ─── Workflow Canvas Stories (from WorkflowHistory data) ─────

export const SimpleLinear: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(simpleLinearWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
SimpleLinear.storyName = "Examples / Simple Linear";

export const Loop: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(loopWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
Loop.storyName = "Examples / Loop";

export const Join: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(joinWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
Join.storyName = "Examples / Join (Parallel)";

export const Race: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(raceWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
Race.storyName = "Examples / Race";

export const FullWorkflow: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(sampleWorkflowHistory);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
FullWorkflow.storyName = "Examples / Full (Complex)";

export const InProgress: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(inProgressWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
InProgress.storyName = "Examples / In Progress";

export const Retry: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(retryWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
Retry.storyName = "Examples / Retrying";

export const Failed: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(failedWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
Failed.storyName = "Examples / Failed";

export const TryControlFlow: Story = () => {
	const { nodes, edges } = workflowHistoryToXYFlow(tryWorkflow);
	return <StoryCanvas nodes={nodes} edges={edges} />;
};
TryControlFlow.storyName = "Examples / Try Control Flow";
