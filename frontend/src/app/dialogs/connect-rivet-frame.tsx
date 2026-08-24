import {
	faCheck,
	faRivet,
	faSpinnerThird,
	faTriangleExclamation,
	Icon,
} from "@rivet-gg/icons";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Suspense, useEffect } from "react";
import {
	AgentPromptBanner,
	CommandBox,
	useComputeInstructionsCode,
} from "@/app/compute-deploy";
import { type DialogContentProps, Frame } from "@/components";
import {
	ErrorDetails,
	useCloudNamespaceDataProvider,
} from "@/components/actors";
import { Button } from "@/components/ui/button";
import { deriveProviderFromMetadata } from "@/lib/data";
import { successfulBackendSetupEffect } from "@/lib/effects";

interface ConnectRivetFrameContentProps extends DialogContentProps {}

export default function ConnectRivetFrameContent({
	onClose,
}: ConnectRivetFrameContentProps) {
	const dataProvider = useCloudNamespaceDataProvider();

	const { data: runnerConfigs } = useInfiniteQuery({
		...dataProvider.runnerConfigsQueryOptions(),
		retry: 0,
		refetchInterval: 3_000,
		maxPages: Infinity,
	});

	const { data: poolData } = useQuery({
		...dataProvider.currentNamespaceManagedPoolQueryOptions({
			pool: "default",
			safe: true,
		}),
		retry: 0,
		refetchInterval: 3_000,
	});

	const hasRunnerConfig = !!runnerConfigs?.find(([, value]) =>
		Object.values(value.datacenters).some(
			(dc) =>
				dc.serverless &&
				deriveProviderFromMetadata(dc.metadata) === "rivet",
		),
	);
	const hasValidPool = !!poolData?.config?.image;
	const isSuccess =
		hasRunnerConfig && hasValidPool && poolData?.status === "ready";

	useEffect(() => {
		if (isSuccess) {
			successfulBackendSetupEffect();
		}
	}, [isSuccess]);

	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					<Icon icon={faRivet} />
					Connect Rivet Cloud
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<div className="flex flex-col gap-6">
					<div className="flex gap-3">
						<StepNumber n={1} />
						<div className="flex-1 min-w-0">
							<p className="font-medium mb-2">
								Deploy your project
							</p>
							<Suspense
								fallback={
									<div className="h-20 animate-pulse bg-muted rounded" />
								}
							>
								<AgentInstructions />
							</Suspense>
						</div>
					</div>
					<div className="flex gap-3">
						<StepNumber n={2} />
						<div className="flex-1 min-w-0">
							<p className="font-medium mb-2">Deploy</p>
							<div className="border rounded-md p-4">
								<PoolStatus
									status={
										hasRunnerConfig && hasValidPool
											? poolData?.status
											: undefined
									}
									error={poolData?.error?.message}
									isSuccess={isSuccess}
								/>
							</div>
						</div>
					</div>
				</div>
				{isSuccess ? (
					<Button className="w-full mt-6" onClick={onClose}>
						Done
					</Button>
				) : null}
			</Frame.Content>
		</>
	);
}

function StepNumber({ n }: { n: number }) {
	return (
		<div className="flex-shrink-0 w-6 h-6 rounded-full bg-primary/10 text-primary text-xs font-medium flex items-center justify-center mt-0.5">
			{n}
		</div>
	);
}

function AgentInstructions() {
	const { code, cloudToken, namespace } = useComputeInstructionsCode();
	const deployCommand = `npx @rivetkit/cli deploy --token ${
		cloudToken ?? "<RIVET_CLOUD_TOKEN>"
	} --namespace ${namespace}`;

	return (
		<div className="flex flex-col gap-3 mt-4">
			<AgentPromptBanner code={code} containsSecret />
			<p className="text-xs text-muted-foreground">
				Or deploy manually from your project root:
			</p>
			<CommandBox command={deployCommand} />
		</div>
	);
}

function PoolStatus({
	status,
	error,
	isSuccess,
}: {
	status?:
		| "ready"
		| "provisioning"
		| "error"
		| "initializing"
		| "allocating"
		| "deploying"
		| "binding"
		| "destroying";
	error?: string;
	isSuccess: boolean;
}) {
	if (isSuccess || status === "ready") {
		return (
			<span>
				<Icon icon={faCheck} className="mr-1.5 text-primary" />
				Deployment successful!
			</span>
		);
	}
	if (status === "provisioning") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Provisioning...
			</span>
		);
	}
	if (status === "error") {
		return (
			<>
				<p>
					<Icon
						icon={faTriangleExclamation}
						className="mr-1.5 text-red-500"
					/>
					Error
				</p>
				<ErrorDetails error={error} />
			</>
		);
	}
	if (status === "initializing") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Initializing...
			</span>
		);
	}
	if (status === "allocating") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Allocating...
			</span>
		);
	}
	if (status === "deploying") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Deploying...
			</span>
		);
	}
	if (status === "binding") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Binding...
			</span>
		);
	}
	if (status === "destroying") {
		return (
			<span>
				<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
				Destroying...
			</span>
		);
	}
	return (
		<span>
			<Icon icon={faSpinnerThird} className="mr-1.5 animate-spin" />
			Waiting for deployment...
		</span>
	);
}
