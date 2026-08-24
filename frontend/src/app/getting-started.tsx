import {
	faActors,
	faArrowRight,
	faCheck,
	faChevronDown,
	faKey,
	Icon,
} from "@rivet-gg/icons";
import { deployOptions, type Provider } from "@rivetkit/shared-data";
import {
	useMutation,
	useQuery,
	useSuspenseInfiniteQuery,
	useSuspenseQuery,
} from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { motion } from "framer-motion";
import { type ReactNode, Suspense, useContext, useMemo } from "react";
import { useFormContext, useWatch } from "react-hook-form";
import { match } from "ts-pattern";
import z from "zod";
import * as ConnectServerfulForm from "@/app/forms/connect-manual-serverful-form";
import * as ConnectServerlessForm from "@/app/forms/connect-manual-serverless-form";
import {
	CodeFrame,
	CodeGroup,
	CodeGroupSyncProvider,
	CodePreview,
	FormField,
	Skeleton,
} from "@/components";
import {
	useCloudNamespaceDataProvider,
	useEngineCompatDataProvider,
} from "@/components/actors";
import { defineStepper } from "@/components/ui/stepper";
import { deriveProviderFromMetadata } from "@/lib/data";
import { engineEnv } from "@/lib/env";
import { features } from "@/lib/features";
import { queryClient } from "@/queries/global";
import { cn } from "../components/lib/utils";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "../components/ui/dropdown-menu";
import { TEST_IDS } from "../utils/test-ids";
import { DeploymentCheck } from "./deployment-check";
import { useEndpoint } from "./dialogs/connect-manual-serverful-frame";
import {
	buildServerlessConfig,
	Configuration,
	ConfigurationAccordion,
} from "./dialogs/connect-manual-serverless-frame";
import { EnvVariables } from "./env-variables";
import { StepperForm, StepVisibilityContext } from "./forms/stepper-form";
import { Content } from "./layout";
import { AgentSelectStep } from "@/components/onboarding/agent-os/agent-select-step";
import { buildAgentOsSetup } from "@/components/onboarding/agent-os/build-agent-os-setup";
import {
	DEFAULT_AGENT,
	DEFAULT_PACKAGES,
	DEFAULT_SANDBOX_PROVIDER,
} from "@/components/onboarding/agent-os/catalog";
import {
	AgentPromptBanner,
	CommandBox,
	defaultRuntimeModeForProvider,
	useAgentInstructionsCode,
	useComputeInstructionsCode,
} from "./compute-deploy";

function platformTitle(provider: unknown): string {
	return (
		deployOptions.find((o) => o.name === provider)?.displayName ??
		"Rivet Compute"
	);
}

const stepper = defineStepper(
	{
		id: "local",
		title: "Run locally",
		titleFor: (values: Record<string, unknown>) =>
			values.template === "agent-os"
				? "What are you building?"
				: "Run locally",
		description: "Get your first Rivet Actor running on your machine.",
		next: "Continue",
		// `template` is carried in the step schema so the stepper accumulates it
		// into its running values. The agentOS steps below gate on it via
		// isVisible, so it must survive navigation past this step.
		schema: z.object({
			template: z.enum(["actor", "agent-os"]).optional(),
		}),
		group: "local",
	},
	// agentOS-only steps. Hidden for the actor path via isVisible, so the
	// stepper skips them and the wizard stays a two-step local -> deploy flow.
	{
		id: "agent",
		title: "Choose your agent",
		description: "Pick the coding agent to run inside agentOS.",
		next: "Continue",
		schema: z.object({ agent: z.string().nonempty() }),
		group: "local",
		isVisible: (values: Record<string, unknown>) =>
			values.template === "agent-os",
	},
	{
		id: "handoff",
		title: "Set up agentOS",
		description:
			"Boot an agentOS instance and run your first session, locally.",
		next: "Continue to deploy",
		schema: z.object({}),
		group: "local",
		isVisible: (values: Record<string, unknown>) =>
			values.template === "agent-os",
	},
	{
		id: "deploy",
		title: "Deploy",
		titleFor: (values: Record<string, unknown>) =>
			`Deploy to ${platformTitle(values.provider)}`,
		next: "Done",
		previous: "Back",
		assist: true,
		group: "deploy",
		schema: (values: Record<string, unknown>) => {
			const provider = (values.provider as string) || "rivet";
			if (provider === "rivet") {
				return z.object({ success: z.literal(true) });
			}
			if ((values.mode as string) === "serverful") {
				return z.object({
					mode: z.literal("serverful"),
					runnerName: z.string().min(1, "Runner name is required"),
					datacenter: z.string().min(1, "Please select a region"),
					customName: z
						.string()
						.trim()
						.max(32, "Name is too long")
						.optional(),
					customIcon: z.string().optional(),
				});
			}
			return z.object({
				mode: z
					.union([z.literal("serverless"), z.literal("serverful")])
					.optional(),
				...ConnectServerlessForm.configurationSchema.shape,
				...ConnectServerlessForm.deploymentSchema.shape,
			});
		},
	},
);

export function GettingStarted({
	displayFrontendOnboarding,
	provider,
}: {
	provider?: Provider;
	displayFrontendOnboarding?: boolean;
}) {
	const dataProvider = useEngineCompatDataProvider();
	useSuspenseInfiniteQuery(dataProvider.datacentersQueryOptions());

	const { data: initialRunnerConfig } = useSuspenseQuery({
		...dataProvider.runnerConfigQueryOptions({
			name: "default",
			safe: true,
		}),
		select: (data) => {
			const config = Object.values(data?.datacenters || {}).find(
				(dc) => dc.serverless,
			);
			const serverlessConfig = config?.serverless;

			if (!serverlessConfig) {
				return null;
			}

			return {
				...serverlessConfig,
				runnerName: "default",
				endpoint: serverlessConfig.url,
				headers: Array.from(
					Object.entries(serverlessConfig.headers || {}),
				),
				provider: deriveProviderFromMetadata(config?.metadata || {}),
				datacenters: Object.fromEntries(
					Object.keys(data.datacenters).map((name) => [name, true]),
				),
			};
		},
	});

	const { mutateAsync } = useMutation({
		...dataProvider.upsertRunnerConfigMutationOptions(),
		onSuccess: async () => {
			await queryClient.invalidateQueries(
				dataProvider.runnerConfigsQueryOptions(),
			);
		},
	});

	const navigate = useNavigate();

	// Rivet Compute is cloud-only (its deploy path consumes cloud-namespace data
	// providers and managed-pool APIs). On flavors without compute, default to
	// the first non-rivet platform so the deploy step renders the engine-backed
	// connect flow instead of crashing on a missing cloud route.
	const defaultProvider = features.compute
		? provider || "rivet"
		: provider && provider !== "rivet"
			? provider
			: (deployOptions.find((o) => o.name !== "rivet")?.name ?? "custom");

	const defaultValues = {
		runnerName: "default",
		headers: [],
		requestLifespan: 900,
		drainGracePeriod: 0,
		provider: defaultProvider,
		datacenters: {},
		datacenter: "",
		// Default the runner mode from the selected provider (serverful for most
		// platforms, serverless for function-only ones). Keep serverless when an
		// existing serverless config was found so its prefill still applies.
		mode: (initialRunnerConfig
			? "serverless"
			: defaultRuntimeModeForProvider(defaultProvider)) as
			| "serverless"
			| "serverful",
		template: "actor" as "actor" | "agent-os",
		agent: DEFAULT_AGENT,
		packages: DEFAULT_PACKAGES,
		sandbox: { enabled: false, provider: DEFAULT_SANDBOX_PROVIDER } as {
			enabled: boolean;
			provider?: string;
		},
		...(initialRunnerConfig || {}),
	};

	// Saves a non-Rivet provider's runner config on finish. The Rivet path
	// deploys via the CLI and writes no config here.
	const saveProviderConfig = async (values: Record<string, unknown>) => {
		const v = values as unknown as {
			runnerName: string;
			provider: string;
			mode?: "serverless" | "serverful";
			datacenter?: string;
			customName?: string;
			customIcon?: string;
		};
		if (v.mode === "serverful") {
			const existingConfig = await queryClient.fetchQuery(
				dataProvider.runnerConfigQueryOptions({
					name: v.runnerName,
					safe: true,
				}),
			);
			const existing = existingConfig?.datacenters || {};
			const isCustom =
				v.provider === "custom" || v.provider === "custom-platform";
			const customName = isCustom
				? v.customName?.trim() || undefined
				: undefined;
			const customIcon = isCustom ? v.customIcon || undefined : undefined;
			await mutateAsync({
				name: v.runnerName,
				config: {
					...existing,
					[v.datacenter as string]: {
						normal: {},
						metadata: {
							provider: v.provider,
							...(customName ? { customName } : {}),
							...(customIcon ? { customIcon } : {}),
						},
					},
				},
			});
		} else {
			const config = await buildServerlessConfig(
				dataProvider,
				values as unknown as Parameters<
					typeof buildServerlessConfig
				>[1],
				{ provider: v.provider },
			);
			await mutateAsync({ name: v.runnerName, config });
		}
	};

	return (
		<Content className="flex-1 min-h-0 !h-auto !overflow-hidden flex flex-col items-center justify-safe-center">
			<motion.div
				className="relative min-w-0 w-full flex-1 min-h-0 flex flex-col"
				initial={{ opacity: 0, y: 20 }}
				animate={{ opacity: 1, y: 0 }}
				transition={{ duration: 0.3 }}
				data-testid={TEST_IDS.Onboarding.GettingStartedWizard}
			>
				<SkipOnboardingHeaderLink />
				<div className="flex-1 min-h-0 overflow-auto flex items-safe-center justify-center px-4 py-8 [&_[data-component='stepper']]:w-full [&_[data-component='stepper']>form]:w-full">
					<div className="relative w-full max-w-[36rem] rounded-xl border bg-card p-6 sm:p-8 shadow-sm">
						<CodeGroupSyncProvider>
							<StepperForm
								{...stepper}
								singlePage
								formId="onboarding"
								className="mt-2"
								header={<OnboardingHeader />}
								initialStep={
									displayFrontendOnboarding || provider
										? "deploy"
										: undefined
								}
								defaultValues={defaultValues}
								content={{
									local: () => (
										<StepContent>
											<RunLocallyStep />
										</StepContent>
									),
									agent: () => (
										<StepContent>
											<AgentSelectStep />
										</StepContent>
									),
									handoff: () => (
										<StepContent>
											<AgentOsHandoff />
										</StepContent>
									),
									deploy: () => (
										<StepContent>
											<Suspense
												fallback={
													<div className="space-y-6">
														<Skeleton className="w-full h-[180px]" />
														<Skeleton className="w-full h-[200px]" />
													</div>
												}
											>
												<DeployScreen />
											</Suspense>
										</StepContent>
									),
								}}
								onSubmit={async ({ values, form }) => {
									// Finish: the deploy step is last. Save a
									// non-Rivet provider's config, then go to the
									// dashboard.
									const accumulated = values as Record<
										string,
										unknown
									>;
									const live = form.getValues() as Record<
										string,
										unknown
									>;
									const provider = (accumulated.provider ??
										live.provider) as string | undefined;
									if (provider && provider !== "rivet") {
										await saveProviderConfig({
											...accumulated,
											provider,
										});
									}
									await navigate({
										to: ".",
										search: (s) => ({
											...s,
											skipOnboarding: true,
										}),
									});
								}}
								onPartialSubmit={async ({ stepper }) => {
									// The managed pool is created by the Rivet CLI
									// during deploy, not by the dashboard, so we only
									// prefetch the data the deploy step renders here.
									if (stepper.current.id === "local") {
										await Promise.all([
											...(features.auth &&
											"publishableTokenQueryOptions" in
												dataProvider
												? [
														queryClient.prefetchQuery(
															dataProvider.publishableTokenQueryOptions(),
														),
														queryClient.prefetchInfiniteQuery(
															dataProvider.datacentersQueryOptions(),
														),
													]
												: []),
											dataProvider.engineAdminTokenQueryOptions(),
										]);
									}
								}}
							>
								<StepperFooter />
							</StepperForm>
						</CodeGroupSyncProvider>
					</div>
				</div>
			</motion.div>
		</Content>
	);
}

function StepContent({ children }: { children: ReactNode }) {
	return (
		<div className="w-full" data-component="step-content">
			{children}
		</div>
	);
}

function StepperFooter() {
	return null;
}

// Header rendered above each step. A full-width segmented progress bar anchors
// the top of the card, with the step label and (on the deploy step) the platform
// switcher on the row below it so nothing competes with the left-aligned title.
function OnboardingHeader() {
	const s = stepper.useStepper();
	return (
		<OnboardingProgress
			action={s.current.id === "deploy" ? <SwitchPlatform /> : null}
		/>
	);
}

// Platform switcher pinned top-right on the deploy screen. Defaults to Rivet
// Compute; selecting another option updates the `provider` form field, which
// re-tunes the deploy screen.
function SwitchPlatform() {
	const { setValue } = useFormContext();
	const provider = (useWatch({ name: "provider" }) as string) || "rivet";
	const options = deployOptions.filter(
		(o) => features.compute || o.name !== "rivet",
	);
	const otherOptions = options.filter((o) => o.name !== provider);
	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>
				<Button
					type="button"
					variant="outline"
					endIcon={<Icon icon={faChevronDown} className="ms-2" />}
				>
					Switch platform
					{otherOptions.length > 0 ? (
						<span className="ms-2 flex items-center -space-x-1.5">
							{otherOptions.slice(0, 4).map((option) => (
								<span
									key={option.name}
									className="flex size-5 items-center justify-center rounded-full border bg-background"
								>
									<Icon
										icon={option.icon}
										className="!size-3 text-muted-foreground"
									/>
								</span>
							))}
						</span>
					) : null}
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent
				side="bottom"
				align="end"
				sideOffset={6}
				className="w-[22rem] max-h-[60vh] overflow-auto"
			>
				{options.map((option) => (
					<DropdownMenuItem
						key={option.name}
						className="items-start gap-3 py-2"
						onClick={() => {
							setValue("provider", option.name, {
								shouldDirty: true,
								shouldTouch: true,
								shouldValidate: true,
							});
							// Reset the runner mode to the new provider's default
							// so switching to a container platform lands on the
							// runner and a function platform lands on serverless.
							setValue(
								"mode",
								defaultRuntimeModeForProvider(option.name),
								{
									shouldDirty: true,
									shouldTouch: true,
									shouldValidate: true,
								},
							);
						}}
					>
						<Icon
							icon={option.icon}
							className="!size-4 mt-0.5 shrink-0 text-muted-foreground"
						/>
						<div className="min-w-0 flex-1">
							<div className="flex items-center gap-2">
								<span className="text-sm font-medium">
									{option.displayName}
								</span>
								{option.badge ? (
									<Badge
										variant="outline"
										className="text-[10px] leading-none py-0.5 px-1.5 font-medium"
									>
										{option.badge}
									</Badge>
								) : null}
								{provider === option.name ? (
									<Icon
										icon={faCheck}
										className="size-3 text-primary"
									/>
								) : null}
							</div>
							<p className="text-xs text-muted-foreground">
								{option.description}
							</p>
						</div>
					</DropdownMenuItem>
				))}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

// Screen 2: deploy. Rivet Compute (default) uses the CLI + a live deploy check;
// other platforms reuse the existing connect setup.
function DeployScreen() {
	const provider = (useWatch({ name: "provider" }) as string) || "rivet";
	// The rivet deploy path is cloud-only; never take it without compute.
	if (provider === "rivet" && features.compute) {
		return <RivetDeploy />;
	}
	return <BackendSetup />;
}

function RivetDeploy() {
	const dataProvider = useCloudNamespaceDataProvider();
	const { data: cloudToken } = useSuspenseQuery(
		dataProvider.createApiTokenQueryOptions({ name: "Onboarding" }),
	);
	const deployCommand = `npx @rivetkit/cli deploy --token ${cloudToken ?? "<RIVET_CLOUD_TOKEN>"}`;
	const isAgentOs = useWatch({ name: "template" }) === "agent-os";
	return (
		<div className="flex flex-col gap-6">
			{isAgentOs ? <AgentOsKeyNotice /> : null}
			<CopyAgentInstructionsButton provider="rivet" />
			<OrDivider label="or deploy manually" />
			<div>
				<p className="text-sm text-muted-foreground mb-3">
					Run this from your project root. The CLI builds and pushes
					your image and provisions Rivet Compute. The token is saved
					to{" "}
					<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">
						~/.rivet/credentials
					</code>{" "}
					so later deploys can omit it.
				</p>
				<CommandBox command={deployCommand} />
			</div>
			<div className="border rounded-lg py-8">
				<div className="flex gap-2 justify-center items-center flex-col py-2 px-8">
					<DeploymentCheck
						validateConfig={(data) =>
							!!data?.find(([, value]) =>
								Object.values(value.datacenters).some(
									(dc) =>
										dc.serverless &&
										deriveProviderFromMetadata(
											dc.metadata,
										) === "rivet",
								),
							)
						}
						validatePool={(data) => !!data?.config?.image}
					/>
				</div>
			</div>
		</div>
	);
}

function SkipOnboardingHeaderLink() {
	if (features.platform) return null;
	return (
		<div className="absolute top-2 right-2 z-10">
			<Button
				asChild
				variant="ghost"
				size="sm"
				className="text-muted-foreground hover:text-foreground"
				endIcon={<Icon icon={faArrowRight} className="ms-1" />}
			>
				<Link to="." search={(s) => ({ ...s, skipOnboarding: true })}>
					Skip onboarding
				</Link>
			</Button>
		</div>
	);
}

function OnboardingProgress({ action }: { action?: ReactNode }) {
	const s = stepper.useStepper();
	const { isStepVisible, visibleStepIndex, visibleStepCount } = useContext(
		StepVisibilityContext,
	);
	// Count only steps visible for the current path (agentOS adds steps that are
	// hidden for the actor path), so "Step X of N" and the dots stay accurate.
	const steps = s.all.filter((step) => isStepVisible(step.id));
	const currentIndex = Math.max(0, visibleStepIndex(s.current.id));
	const total = visibleStepCount;
	const groupLabel = s.current.group === "local" ? "Local setup" : "Deploy";
	return (
		<div className="mb-6 flex flex-col gap-2">
			<div
				role="progressbar"
				aria-valuemin={1}
				aria-valuemax={total}
				aria-valuenow={currentIndex + 1}
				aria-valuetext={`Step ${currentIndex + 1} of ${total}, ${groupLabel}`}
				className="flex gap-1.5"
			>
				{steps.map((step, i) => (
					<div
						key={step.id}
						className={cn(
							"h-1 flex-1 rounded-full transition-colors",
							i <= currentIndex ? "bg-primary" : "bg-muted",
						)}
					/>
				))}
			</div>
			<div className="flex min-h-8 items-center justify-between gap-4">
				<div className="text-xs text-muted-foreground tabular-nums">
					Step {currentIndex + 1} of {total} · {groupLabel}
				</div>
				{action}
			</div>
		</div>
	);
}

function OrDivider({ label }: { label: string }) {
	return (
		<div className="flex items-center gap-3">
			<div className="h-px flex-1 bg-border" />
			<span className="text-xs text-muted-foreground">{label}</span>
			<div className="h-px flex-1 bg-border" />
		</div>
	);
}

function AgentOsKeyNotice() {
	return (
		<div className="flex gap-3 rounded-md border border-border bg-muted/30 p-3 text-sm">
			<Icon
				icon={faKey}
				className="mt-0.5 shrink-0 text-muted-foreground"
			/>
			<div className="space-y-1">
				<p className="font-medium">agentOS needs an LLM key</p>
				<p className="text-muted-foreground text-xs">
					Set{" "}
					<code className="rounded bg-muted px-1 py-0.5 text-foreground">
						ANTHROPIC_API_KEY
					</code>{" "}
					as a secret on your deployment. agentOS doesn't inherit it
					from the host process, so your server passes it to each
					session.{" "}
					<a
						href="https://agentos-sdk.dev/docs/llm-credentials/"
						target="_blank"
						rel="noreferrer"
						className="text-primary hover:underline"
					>
						Learn more
					</a>
				</p>
			</div>
		</div>
	);
}

// agentOS brand mark (rounded square + "OS") drawn in currentColor so it adapts
// to the theme, unlike the white-only marketing SVG.
function AgentOsLogo({ className }: { className?: string }) {
	return (
		<svg
			viewBox="0 0 32 32"
			fill="none"
			className={className}
			aria-hidden="true"
		>
			<rect
				x="2.75"
				y="2.75"
				width="26.5"
				height="26.5"
				rx="8"
				stroke="currentColor"
				strokeWidth="2.5"
			/>
			<text
				x="16"
				y="20.5"
				textAnchor="middle"
				fontSize="11"
				fontWeight="700"
				fontFamily="inherit"
				fill="currentColor"
			>
				OS
			</text>
		</svg>
	);
}

function BuildTargetCard({
	icon,
	label,
	description,
	badge,
	isSelected,
	onSelect,
}: {
	icon: ReactNode;
	label: string;
	description: string;
	badge?: string;
	isSelected: boolean;
	onSelect: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onSelect}
			className={cn(
				"flex items-start gap-3 rounded-lg border px-4 py-3 text-left transition-colors cursor-pointer",
				isSelected
					? "border-primary bg-primary/5"
					: "border-border hover:border-muted-foreground/50",
			)}
		>
			<span className="text-muted-foreground mt-0.5 shrink-0">
				{icon}
			</span>
			<div className="min-w-0">
				<div className="flex items-center gap-2">
					<p className="text-sm font-medium">{label}</p>
					{badge ? (
						<Badge
							variant="outline"
							className="text-[10px] leading-none py-0.5 px-1.5 font-medium"
						>
							{badge}
						</Badge>
					) : null}
				</div>
				<p className="text-xs text-muted-foreground">{description}</p>
			</div>
		</button>
	);
}

// "What are you building?" selector shown atop the first step when the agentOS
// feature flag is on. Picking agentOS reveals the agent/software/sandbox/handoff
// steps (gated by `template === "agent-os"` via the stepper's isVisible).
function BuildTargetSelector() {
	const { control, setValue } = useFormContext();
	return (
		<FormField
			control={control}
			name="template"
			render={({ field }) => (
				<div>
					<p className="font-medium mb-2">What are you building?</p>
					<div className="grid grid-cols-2 gap-2">
						<BuildTargetCard
							icon={<Icon icon={faActors} className="!size-5" />}
							label="Rivet Actors"
							description="Realtime, state, and multiplayer for any app"
							isSelected={field.value !== "agent-os"}
							onSelect={() =>
								setValue("template", "actor", {
									shouldDirty: true,
									shouldTouch: true,
									shouldValidate: true,
								})
							}
						/>
						<BuildTargetCard
							icon={<AgentOsLogo className="size-5" />}
							label="agentOS"
							badge="Beta"
							description="An open-source OS for agents. Runs in-process with ~6 ms cold starts."
							isSelected={field.value === "agent-os"}
							onSelect={() =>
								setValue("template", "agent-os", {
									shouldDirty: true,
									shouldTouch: true,
									shouldValidate: true,
								})
							}
						/>
					</div>
				</div>
			)}
		/>
	);
}

function RunLocallyStep() {
	const isAgentOs = useWatch({ name: "template" }) === "agent-os";
	return (
		<div className="flex flex-col gap-6">
			{features.agentOs ? <BuildTargetSelector /> : null}
			{isAgentOs ? null : (
				<>
					{features.compute ? (
						<RunLocallyComputeBanner />
					) : (
						<RunLocallyGenericBanner />
					)}
					<OrDivider label="or do it yourself" />
					<div className="w-full flex items-center justify-between gap-4 rounded-lg px-4 py-4 border border-border">
						<div className="min-w-0">
							<p className="font-medium mb-1">
								Follow the quickstart guide
							</p>
							<p className="text-sm text-muted-foreground">
								Build a Rivet Actor project by hand, step by
								step.
							</p>
						</div>
						<Button variant="outline" asChild className="shrink-0">
							<a
								href="https://rivet.dev/docs/actors/quickstart/"
								target="_blank"
								rel="noopener noreferrer"
							>
								Quickstart guide
								<Icon icon={faArrowRight} className="ms-2" />
							</a>
						</Button>
					</div>
				</>
			)}
		</div>
	);
}

// agentOS handoff: turns the agent/software/sandbox selections into the install
// command, server.ts/client.ts, and a copy-prompt for the coding agent. Shown
// as the final local-group step on the agentOS path.
function AgentOsHandoff() {
	const agent = useWatch({ name: "agent" }) as string | undefined;
	const packages = useWatch({ name: "packages" }) as string[] | undefined;
	const sandbox = useWatch({ name: "sandbox" }) as
		| { enabled: boolean; provider?: string }
		| undefined;

	const setup = useMemo(
		() =>
			buildAgentOsSetup({
				agent: agent ?? DEFAULT_AGENT,
				packages: packages ?? DEFAULT_PACKAGES,
				sandbox: sandbox ?? {
					enabled: false,
					provider: DEFAULT_SANDBOX_PROVIDER,
				},
			}),
		[agent, packages, sandbox],
	);

	return (
		<div className="flex flex-col gap-6">
			<AgentPromptBanner
				code={setup.prompt}
				title="Use your coding agent"
				description="Have your coding agent set up agentOS and run a session for you."
			/>
			<OrDivider label="or set it up yourself" />
			<div className="flex flex-col gap-4">
				<div>
					<p className="font-medium mb-1.5">Install</p>
					<CommandBox command={setup.installCommand} />
				</div>
				<CodeGroup className="my-0">
					{[
						<CodeFrame
							key="server"
							language="typescript"
							title="server.ts"
							code={() => setup.serverCode}
							className="m-0"
						>
							<CodePreview
								language="typescript"
								className="text-left"
								code={setup.serverCode}
							/>
						</CodeFrame>,
						<CodeFrame
							key="client"
							language="typescript"
							title="client.ts"
							code={() => setup.clientCode}
							className="m-0"
						>
							<CodePreview
								language="typescript"
								className="text-left"
								code={setup.clientCode}
							/>
						</CodeFrame>,
					]}
				</CodeGroup>
			</div>
		</div>
	);
}

// Compute is the default deploy target, so the run-locally prompt ships the
// compute deployment addendum alongside the onboarding instructions. Copying it
// gives the agent everything it needs to scaffold, run, and deploy in one paste.
function RunLocallyComputeBanner() {
	const { code } = useComputeInstructionsCode();
	return (
		<AgentPromptBanner
			code={code}
			containsSecret
			title="Use your coding agent"
			description="Copy a prompt that scaffolds, runs, and deploys your first Actor for you."
		/>
	);
}

function RunLocallyGenericBanner() {
	const code = useAgentInstructionsCode();
	return (
		<AgentPromptBanner
			code={code}
			title="Use your coding agent"
			description="Copy a prompt that scaffolds and runs your first Actor for you."
		/>
	);
}

function StepNumber({ n }: { n: number }) {
	return (
		<div className="flex-shrink-0 w-6 h-6 rounded-full bg-primary/10 text-primary text-xs font-medium flex items-center justify-center mt-0.5">
			{n}
		</div>
	);
}

function CopyAgentInstructionsButton({ provider }: { provider?: Provider }) {
	// The compute prompt reads cloud-namespace data; only available with compute.
	if (provider === "rivet" && features.compute) {
		return <ComputeCopyAgentInstructionsButton />;
	}
	return <GenericCopyAgentInstructionsButton provider={provider} />;
}

function ComputeCopyAgentInstructionsButton() {
	const { code } = useComputeInstructionsCode();
	return <AgentPromptBanner code={code} containsSecret />;
}

function GenericCopyAgentInstructionsButton({
	provider,
}: {
	provider?: Provider;
}) {
	const endpoint = useEndpoint();
	const runnerName = useWatch({ name: "runnerName" }) as string;
	const mode = useWatch({ name: "mode" }) as
		| "serverless"
		| "serverful"
		| undefined;
	const code = useAgentInstructionsCode({
		provider,
		runnerName,
		endpoint,
		mode,
	});
	return (
		<AgentPromptBanner
			code={code}
			containsSecret
			description={`Have your coding agent complete these steps to deploy to ${platformTitle(provider)}.`}
		/>
	);
}

const githubActionYaml = `name: Rivet Deploy

on:
  pull_request:
    types: [opened, synchronize, reopened, closed]
  push:
    branches: [main]
  workflow_dispatch:

concurrency:
  group: rivet-deploy-\${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: true

jobs:
  rivet-deploy:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
      - uses: rivet-dev/deploy-action@v1.1.2
        with:
          rivet-token: \${{ secrets.RIVET_CLOUD_TOKEN }}`;

function BackendSetupRivet() {
	const dataProvider = useCloudNamespaceDataProvider();
	const { data: cloudToken } = useSuspenseQuery(
		dataProvider.createApiTokenQueryOptions({ name: "Onboarding" }),
	);

	const ghSecretCmd = cloudToken
		? `gh secret set RIVET_CLOUD_TOKEN --body "${cloudToken}"`
		: "gh secret set RIVET_CLOUD_TOKEN";
	const isAgentOs = useWatch({ name: "template" }) === "agent-os";

	return (
		<div className="flex flex-col gap-6">
			{isAgentOs ? <AgentOsKeyNotice /> : null}
			<CopyAgentInstructionsButton provider="rivet" />
			<OrDivider label="or set it up manually" />
			<div className="flex gap-3">
				<StepNumber n={1} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">Create a Dockerfile</p>
					<p className="text-sm text-muted-foreground">
						Add a{" "}
						<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">
							Dockerfile
						</code>{" "}
						to the root of your project that builds and runs your
						RivetKit server.
					</p>
				</div>
			</div>
			<div className="flex gap-3">
				<StepNumber n={2} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">Add GitHub secret</p>
					<p className="text-sm text-muted-foreground mb-3">
						Add your Rivet token as a repository secret named{" "}
						<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">
							RIVET_CLOUD_TOKEN
						</code>
						.
					</p>
					<CodeGroup className="my-0">
						{[
							<CodeFrame
								key="gh-secret"
								language="bash"
								title="bash"
								code={() => ghSecretCmd}
								className="m-0"
							>
								<CodePreview
									language="bash"
									className="text-left"
									code={ghSecretCmd}
								/>
							</CodeFrame>,
						]}
					</CodeGroup>
				</div>
			</div>
			<div className="flex gap-3">
				<StepNumber n={3} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">Add GitHub Action</p>
					<p className="text-sm text-muted-foreground mb-3">
						Create{" "}
						<code className="font-mono text-xs bg-muted px-1 py-0.5 rounded">
							.github/workflows/rivet-deploy.yml
						</code>{" "}
						to automatically deploy on every push and pull request.
					</p>
					<CodeGroup className="my-0">
						{[
							<CodeFrame
								key="gh-action"
								language="yaml"
								title=".github/workflows/rivet-deploy.yml"
								code={() => githubActionYaml}
								className="m-0"
							>
								<CodePreview
									language="yaml"
									className="text-left"
									code={githubActionYaml}
								/>
							</CodeFrame>,
						]}
					</CodeGroup>
				</div>
			</div>
			<div className="flex gap-3">
				<StepNumber n={4} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">Deploy to Rivet Compute</p>
					<p className="text-sm text-muted-foreground mb-2">
						Push your changes to trigger the{" "}
						<strong>Rivet Deploy</strong> workflow. The status check
						below will update automatically once your backend is
						deployed.
					</p>
					<div className="border rounded-md py-8">
						<div className="flex gap-2 justify-center items-center flex-col py-2 px-8">
							<DeploymentCheck
								validateConfig={(data) =>
									!!data?.find(([, value]) =>
										Object.values(value.datacenters).some(
											(dc) =>
												dc.serverless &&
												deriveProviderFromMetadata(
													dc.metadata,
												) === "rivet",
										),
									)
								}
								validatePool={(data) => !!data?.config?.image}
							/>
						</div>
					</div>
				</div>
			</div>
		</div>
	);
}

function BackendSetup() {
	const provider = useWatch({ name: "provider" });
	const isAgentOs = useWatch({ name: "template" }) === "agent-os";
	const rawMode = useWatch({ name: "mode" }) as
		| "serverless"
		| "serverful"
		| undefined;
	// The runner mode is fixed per provider (no user toggle); render the mode
	// that provider deploys with.
	const mode = rawMode ?? defaultRuntimeModeForProvider(provider);

	if (provider === "rivet" && features.compute) {
		return <BackendSetupRivet />;
	}

	return (
		<div className="flex flex-col gap-6">
			{isAgentOs ? <AgentOsKeyNotice /> : null}
			<CopyAgentInstructionsButton provider={provider} />
			<OrDivider label="or set it up manually" />
			{mode === "serverful" ? (
				<BackendSetupServerful provider={provider} />
			) : (
				<BackendSetupServerless provider={provider} />
			)}
		</div>
	);
}

function BackendSetupServerless({ provider }: { provider: Provider }) {
	const endpoint = useWatch({ name: "endpoint" });
	const isCustom = provider === "custom" || provider === "custom-platform";
	return (
		<>
			<div className="flex gap-3">
				<StepNumber n={1} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">
						Set environment variables
					</p>
					<p className="text-sm text-muted-foreground mb-3">
						Configure the following environment variables in your
						deployment.
					</p>
					<div className="space-y-2">
						<EnvVariables endpoint={endpoint ?? ""} />
					</div>
				</div>
			</div>
			<div className="flex gap-3">
				<StepNumber n={2} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-4">
						Paste your deployment endpoint
					</p>
					<div className="space-y-2">
						<Configuration
							runnerName={false}
							datacenters
							headers={false}
							requestLifespan={false}
							drainGracePeriod={false}
						/>
						<ConnectServerlessForm.Endpoint
							placeholder={match(provider)
								.with(
									"vercel",
									() =>
										"https://your-vercel-deployment.vercel.app",
								)
								.with(
									"railway",
									() => "https://your-app.up.railway.app",
								)
								.otherwise(() => "https://your-deployment.com")}
						/>
						<ConfigurationAccordion
							datacenters={false}
							prefixFields={
								isCustom ? (
									<ConnectServerlessForm.CustomBranding />
								) : null
							}
						/>
						<ConnectServerlessForm.ConnectionCheck
							provider={provider}
						/>
					</div>
				</div>
			</div>
		</>
	);
}

function BackendSetupServerful({ provider }: { provider: Provider }) {
	const isCustom = provider === "custom" || provider === "custom-platform";
	const endpoint = useServerfulEndpoint();
	const runnerName = useWatch({ name: "runnerName" });

	return (
		<>
			<div className="flex gap-3">
				<StepNumber n={1} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-4">Configure your runner</p>
					<div className="space-y-3">
						<ConnectServerfulForm.RunnerName />
						{isCustom ? (
							<ConnectServerlessForm.CustomBranding />
						) : null}
						<ConnectServerfulForm.Datacenter />
					</div>
				</div>
			</div>
			<div className="flex gap-3">
				<StepNumber n={2} />
				<div className="flex-1 min-w-0">
					<p className="font-medium mb-2">
						Set environment variables
					</p>
					<p className="text-sm text-muted-foreground mb-3">
						Set the following environment variables on the machine
						running your runner.
					</p>
					<div className="space-y-2">
						<EnvVariables
							endpoint={endpoint}
							runnerName={runnerName}
							showPublicEndpoint={false}
						/>
					</div>
				</div>
			</div>
		</>
	);
}

function useServerfulEndpoint() {
	const datacenter = useWatch({ name: "datacenter" });
	const dataProvider = useEngineCompatDataProvider();
	const { data } = useQuery(
		dataProvider.datacenterQueryOptions(datacenter || "auto"),
	);
	return data?.url || engineEnv().VITE_APP_API_URL;
}
