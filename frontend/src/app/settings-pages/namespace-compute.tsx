import type { Rivet } from "@rivet-gg/cloud";
import { faCircleExclamation, faTrash, Icon } from "@rivet-gg/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { type ReactNode, useEffect, useState } from "react";
import { useFormState } from "react-hook-form";
import z from "zod";
import { resolvePoolName } from "@/app/pool-switcher";
import {
	Alert,
	AlertDescription,
	AlertTitle,
	Button,
	Code,
	cn,
	createSchemaForm,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
	Input,
	Skeleton,
	SmallText,
} from "@/components";
import { useCloudNamespaceDataProvider } from "@/components/actors";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { features } from "@/lib/features";
import { SettingsCard } from "./settings-card";

const MEMORY_RE = /^(\d+)(Mi|Gi)$/;

function parseMemoryMib(memory: string): number | null {
	const match = MEMORY_RE.exec(memory);
	if (!match) return null;
	return Number(match[1]) * (match[2] === "Gi" ? 1024 : 1);
}

// Formats pool args for the editable field, quoting entries that contain
// whitespace, quotes, or backslashes so the round-trip through `parseArgs`
// is lossless.
function formatArgs(args: string[]): string {
	return args
		.map((arg) =>
			arg === "" || /[\s"'\\]/.test(arg)
				? `"${arg.replace(/[\\"]/g, "\\$&")}"`
				: arg,
		)
		.join(" ");
}

// Splits an args string on whitespace with shell-style quoting: single
// quotes group a literal token, double quotes group a token with `\"` and
// `\\` escapes, and a backslash outside quotes escapes the next character.
// Returns null on an unterminated quote so the schema can reject the input.
function parseArgs(input: string): string[] | null {
	const args: string[] = [];
	let current = "";
	let hasToken = false;
	let i = 0;
	while (i < input.length) {
		const char = input[i];
		if (/\s/.test(char)) {
			if (hasToken) {
				args.push(current);
				current = "";
				hasToken = false;
			}
			i++;
		} else if (char === "'") {
			const end = input.indexOf("'", i + 1);
			if (end === -1) return null;
			current += input.slice(i + 1, end);
			hasToken = true;
			i = end + 1;
		} else if (char === '"') {
			i++;
			let closed = false;
			while (i < input.length) {
				if (
					input[i] === "\\" &&
					(input[i + 1] === '"' || input[i + 1] === "\\")
				) {
					current += input[i + 1];
					i += 2;
				} else if (input[i] === '"') {
					closed = true;
					i++;
					break;
				} else {
					current += input[i];
					i++;
				}
			}
			if (!closed) return null;
			hasToken = true;
		} else if (char === "\\" && i + 1 < input.length) {
			current += input[i + 1];
			hasToken = true;
			i += 2;
		} else {
			current += char;
			hasToken = true;
			i++;
		}
	}
	if (hasToken) args.push(current);
	return args;
}

// A CPU value the deploy API accepts: the whole vCPU stops 1, 2, 4, and 8,
// or a fractional value below 1 with at most two decimals.
function isCatalogCpu(cpu: number): boolean {
	if (cpu === 1 || cpu === 2 || cpu === 4 || cpu === 8) return true;
	const scaled = cpu * 100;
	return cpu < 1 && Math.abs(scaled - Math.round(scaled)) < 1e-9;
}

function isValidCpu(cpu: number): boolean {
	return Number.isFinite(cpu) && cpu >= 0.08 && cpu <= 8 && isCatalogCpu(cpu);
}

// Constraints mirror the client-side validation of `rivet deploy` in
// engine/packages/cli/src/util.rs::build_resources and
// resolve_max_concurrent_actors.
const computeFormSchema = z
	.object({
		maxConcurrentActors: z.coerce
			.number()
			.int("Must be a whole number")
			.min(1, "Must be between 1 and 50,000")
			.max(50000, "Must be between 1 and 50,000"),
		drainGracePeriod: z.coerce
			.number()
			.int("Must be a whole number")
			.min(5, "Must be between 5 and 3,200")
			.max(3200, "Must be between 5 and 3,200"),
		drainOnVersionUpgrade: z.boolean(),
		command: z.string(),
		args: z
			.string()
			.refine((args) => parseArgs(args) !== null, "Unterminated quote"),
		minScale: z.coerce
			.number()
			.int("Must be a whole number")
			.min(0, "Must be between 0 and 100")
			.max(100, "Must be between 0 and 100"),
		maxScale: z.coerce
			.number()
			.int("Must be a whole number")
			.min(1, "Must be between 1 and 5,000")
			.max(5000, "Must be between 1 and 5,000"),
		cpu: z.coerce
			.number()
			.refine(
				(cpu) => Number.isFinite(cpu) && cpu >= 0.08 && cpu <= 8,
				"Must be between 0.08 and 8",
			)
			.refine(
				isCatalogCpu,
				"Must be 1, 2, 4, or 8, or a value below 1 with at most two decimals",
			),
		memory: z
			.string()
			.refine(
				(memory) => MEMORY_RE.test(memory),
				"Must match <number>Mi or <number>Gi, e.g. 512Mi",
			)
			.refine((memory) => {
				const mib = parseMemoryMib(memory);
				return mib != null && mib >= 512 && mib <= 4096;
			}, "Must be between 512Mi and 4Gi"),
		instanceRequestConcurrency: z.coerce
			.number()
			.int("Must be a whole number")
			.min(1, "Must be between 1 and 500")
			.max(500, "Must be between 1 and 500"),
	})
	.superRefine((values, ctx) => {
		if (values.minScale > values.maxScale) {
			const message = "Min scale must not exceed max scale";
			ctx.addIssue({ code: "custom", path: ["minScale"], message });
			ctx.addIssue({ code: "custom", path: ["maxScale"], message });
		}
	});

type ComputeFormValues = z.infer<typeof computeFormSchema>;

const {
	Form,
	Submit,
	Reset,
	useContext: useComputeForm,
} = createSchemaForm(computeFormSchema);

// The namespace data provider can briefly resolve to `undefined` when the
// user switches namespace/project from the nav while this drawer is open.
// Bail until the new route's loader has populated `dataProvider`. The
// outer/inner split keeps the inner component's hook order stable while the
// route match changes, mirroring `ComputeNavItem` in settings-drawer.tsx.
export function NamespaceComputeContent() {
	const dataProvider = useCloudNamespaceDataProvider();
	if (!dataProvider) {
		return null;
	}
	return <NamespaceComputeContentInner />;
}

function NamespaceComputeContentInner() {
	const dataProvider = useCloudNamespaceDataProvider();

	const { pool: poolParam } = useSearch({ strict: false });
	const { data: pools = [] } = useQuery(
		dataProvider.currentNamespaceManagedPoolsQueryOptions(),
	);
	const selectedPool = resolvePoolName(pools, poolParam);
	// Only non-default pools are deletable (the default anchors resolution) and
	// only when more than one exists; gated on danger-zone like other
	// destructive surfaces.
	const defaultPool = resolvePoolName(pools, undefined);
	const canDeletePool =
		features.dangerZone && pools.length > 1 && selectedPool !== defaultPool;

	const {
		data: pool,
		isPending,
		isError,
	} = useQuery({
		...dataProvider.currentNamespaceManagedPoolQueryOptions({
			pool: selectedPool,
		}),
		// Poll while a deploy is in flight so the footer button tracks the
		// pool status; back off entirely once the pool settles.
		refetchInterval: (query) =>
			isPoolBusy(query.state.data?.status) ? 3_000 : false,
	});

	if (isPending) {
		return (
			<div className="space-y-4">
				<Skeleton className="h-44 w-full rounded-lg" />
				<Skeleton className="h-44 w-full rounded-lg" />
			</div>
		);
	}

	if (isError || !pool?.config) {
		return (
			<div className="space-y-4">
				{pool?.status === "error" ? (
					<PoolErrorAlert error={pool.error} />
				) : null}
				<SettingsCard title="Deployment">
					<SmallText className="text-muted-foreground">
						Failed to load the Rivet Compute deployment for this
						namespace.
					</SmallText>
				</SettingsCard>
			</div>
		);
	}

	return (
		// Remount on pool switch; RHF only reads defaultValues at mount, so
		// without this the form keeps the previous pool's values.
		<ComputeForm
			key={selectedPool}
			pool={selectedPool}
			defaultPool={defaultPool}
			canDelete={canDeletePool}
			config={pool.config}
			status={pool.status}
			error={pool.error}
		/>
	);
}

// Inline delete control for a non-default pool. Uses the same destroy endpoint
// as `rivet pool delete`, behind a confirm click like the actor destroy button.
function DeletePoolButton({
	pool,
	defaultPool,
}: {
	pool: string;
	defaultPool: string;
}) {
	const dataProvider = useCloudNamespaceDataProvider();
	const navigate = useNavigate();
	const queryClient = useQueryClient();
	const [isConfirming, setIsConfirming] = useState(false);

	const { mutate, isPending } = useMutation({
		...dataProvider.deleteCurrentNamespaceManagedPoolMutationOptions(),
		onSuccess: async () => {
			// Switch back to the default pool before the deleted pool's query
			// 404s, then refresh the list so the switcher drops it.
			await navigate({
				to: ".",
				search: (old) => ({
					...(old as Record<string, unknown>),
					pool: defaultPool,
				}),
			});
			await queryClient.invalidateQueries(
				dataProvider.currentNamespaceManagedPoolsQueryOptions(),
			);
		},
	});

	// Reset the confirm prompt if the user walks away without confirming,
	// matching the actor destroy button's dwell window.
	useEffect(() => {
		if (!isConfirming) return;
		const timer = setTimeout(() => setIsConfirming(false), 4000);
		return () => clearTimeout(timer);
	}, [isConfirming]);

	return (
		<Button
			type="button"
			isLoading={isPending}
			variant="destructive-outline"
			size="sm"
			startIcon={isConfirming ? undefined : <Icon icon={faTrash} />}
			onClick={(e) => {
				e?.stopPropagation();
				if (e?.shiftKey || isConfirming) {
					mutate({ pool });
					return;
				}
				setIsConfirming(true);
			}}
		>
			{isPending
				? "Deleting..."
				: isConfirming
					? "Are you sure? This cannot be undone."
					: "Delete pool"}
		</Button>
	);
}

function isPoolBusy(
	status: Rivet.ManagedPoolsGetResponse.ManagedPool.Status | undefined,
) {
	return (
		status === "initializing" ||
		status === "allocating" ||
		status === "deploying" ||
		status === "binding" ||
		status === "destroying"
	);
}

// Surfaces the pool's error state at the top of the tab so a failed deploy
// is visible without opening the CLI or logs.
function PoolErrorAlert({
	error,
}: {
	error: Rivet.ManagedPoolsGetResponse.ManagedPool.Error_ | undefined;
}) {
	return (
		<Alert variant="destructive">
			<Icon icon={faCircleExclamation} className="size-4" />
			<AlertTitle>Deployment failed</AlertTitle>
			<AlertDescription>
				{error?.message ??
					"The compute pool is in an error state. Redeploy to retry."}
			</AlertDescription>
		</Alert>
	);
}

function ComputeForm({
	pool,
	defaultPool,
	canDelete,
	config,
	status,
	error,
}: {
	pool: string;
	defaultPool: string;
	canDelete: boolean;
	config: Rivet.ManagedPoolsGetResponse.ManagedPool.Config;
	status: Rivet.ManagedPoolsGetResponse.ManagedPool.Status;
	error: Rivet.ManagedPoolsGetResponse.ManagedPool.Error_ | undefined;
}) {
	const dataProvider = useCloudNamespaceDataProvider();
	const queryClient = useQueryClient();
	const { mutateAsync } = useMutation(
		dataProvider.upsertCurrentNamespaceManagedPoolMutationOptions(),
	);

	const resources = config.resources;
	const runnerConfig = config.runnerConfig;

	// Unset fields fall back to the server-side defaults so submitting an
	// untouched field pins the value the pool already effectively runs with.
	// The deprecated top-level maxConcurrentActors seeds the runner config
	// value for pools that predate the runnerConfig object.
	const defaultValues: ComputeFormValues = {
		maxConcurrentActors:
			runnerConfig?.maxConcurrentActors ??
			config.maxConcurrentActors ??
			1000,
		drainGracePeriod: runnerConfig?.drainGracePeriod ?? 1800,
		drainOnVersionUpgrade: runnerConfig?.drainOnVersionUpgrade ?? true,
		command: config.command ?? "",
		args: formatArgs(config.args ?? []),
		minScale: resources?.minScale ?? 0,
		maxScale: resources?.maxScale ?? 1,
		cpu: resources?.cpu ?? 1,
		memory: resources?.memory ?? "512Mi",
		instanceRequestConcurrency: resources?.instanceRequestConcurrency ?? 80,
	};

	return (
		<Form
			defaultValues={defaultValues}
			mode="onChange"
			revalidateMode="onChange"
			delayError={100}
			onSubmit={async (values, form) => {
				// The upsert merges field-wise: an omitted field keeps the
				// pool's current value and an explicit null clears it. The
				// command/args overrides are therefore omitted when unchanged
				// and sent as null when the user cleared them. `image` is
				// always omitted so the pool keeps its current build,
				// matching `rivet deploy --reuse-image`.
				const command = values.command.trim();
				const args = parseArgs(values.args);
				if (args === null) {
					throw new Error(
						"args failed to parse after schema validation",
					);
				}
				const initialArgs = config.args ?? [];
				const overrides: {
					command?: string | null;
					args?: string[] | null;
				} = {};
				if (command !== (config.command ?? "").trim()) {
					overrides.command = command === "" ? null : command;
				}
				if (
					args.length !== initialArgs.length ||
					args.some((arg, index) => arg !== initialArgs[index])
				) {
					overrides.args = args.length === 0 ? null : args;
				}
				await mutateAsync({
					pool,
					displayName: config.displayName || "Default",
					runnerConfig: {
						maxConcurrentActors: values.maxConcurrentActors,
						drainGracePeriod: values.drainGracePeriod,
						drainOnVersionUpgrade: values.drainOnVersionUpgrade,
					},
					// The generated SDK request type does not model the
					// clear-on-null semantics, so the null-capable overrides
					// are cast back to the generated field types.
					...(overrides as { command?: string; args?: string[] }),
					resources: {
						minScale: values.minScale,
						maxScale: values.maxScale,
						cpu: values.cpu,
						memory: values.memory,
						instanceRequestConcurrency:
							values.instanceRequestConcurrency,
					},
				});
				await queryClient.invalidateQueries(
					dataProvider.currentNamespaceManagedPoolQueryOptions({
						pool,
					}),
				);
				form.reset(values);
			}}
		>
			<div className="space-y-4">
				{status === "error" ? <PoolErrorAlert error={error} /> : null}
				<SettingsCard
					divided
					title="Deployment"
					description="The container deployed to Rivet Compute for this namespace. The image is updated by running a deploy, not from here."
					contentClassName="divide-y divide-foreground/10"
				>
					<FieldRow
						label={<p className={FIELD_LABEL_CLASS}>Image</p>}
					>
						{config.image ? (
							<Code className="text-right">
								{config.image.repository}:{config.image.tag}
							</Code>
						) : (
							<span className="text-sm text-muted-foreground">
								None
							</span>
						)}
					</FieldRow>
					<TextField name="command" label="Command override" />
					<TextField name="args" label="Args override" />
				</SettingsCard>
				<SettingsCard
					divided
					title="Resources"
					description="Scaling limits and per-instance resources for this deployment."
					contentClassName="divide-y divide-foreground/10"
				>
					<NumberField name="minScale" label="Min scale" />
					<NumberField name="maxScale" label="Max scale" />
					<CpuField />
					<TextField
						name="memory"
						label="Memory"
						placeholder="512Mi"
						narrow
					/>
					<NumberField
						name="instanceRequestConcurrency"
						label="Max actors per container"
					/>
				</SettingsCard>
				<SettingsCard
					divided
					title="Runner Config"
					description="How the pool's runners handle actor capacity and draining."
					contentClassName="divide-y divide-foreground/10"
				>
					<NumberField
						name="maxConcurrentActors"
						label="Max concurrent actors"
					/>
					<NumberField
						name="drainGracePeriod"
						label="Drain grace period (seconds)"
					/>
					<SwitchField
						name="drainOnVersionUpgrade"
						label="Drain on version upgrade"
					/>
				</SettingsCard>
				<DeployFooter
					status={status}
					pool={pool}
					defaultPool={defaultPool}
					canDelete={canDelete}
				/>
			</div>
		</Form>
	);
}

// Delete/discard/deploy controls. The deploy side only shows once the form is
// dirty or a deploy is in flight; the delete button sits on the left.
function DeployFooter({
	status,
	pool,
	defaultPool,
	canDelete,
}: {
	status: Rivet.ManagedPoolsGetResponse.ManagedPool.Status;
	pool: string;
	defaultPool: string;
	canDelete: boolean;
}) {
	const { isDirty } = useFormState<ComputeFormValues>();

	// A tearing-down pool is not a deploy: show "Deleting..." instead of the
	// deploy button so the footer never reads "Deploying..." during teardown.
	if (status === "destroying") {
		return (
			<div className="flex items-center justify-end">
				<span className="text-sm font-medium text-destructive">
					Deleting...
				</span>
			</div>
		);
	}

	const busy = isPoolBusy(status);
	const showDeploy = isDirty || busy;
	if (!showDeploy && !canDelete) return null;
	return (
		<div className="flex items-center justify-between gap-2">
			{canDelete ? (
				<DeletePoolButton pool={pool} defaultPool={defaultPool} />
			) : (
				<span />
			)}
			{showDeploy ? (
				<div className="flex items-center gap-2">
					{isDirty ? (
						<Reset variant="secondary" size="sm">
							Discard
						</Reset>
					) : null}
					<Submit
						size="sm"
						{...(busy ? { disabled: true, isLoading: true } : {})}
					>
						{busy ? "Deploying..." : "Deploy changes"}
					</Submit>
				</div>
			) : null}
		</div>
	);
}

const FIELD_LABEL_CLASS = "pt-2 text-sm font-normal text-muted-foreground";

function FieldRow({
	label,
	children,
}: {
	label: ReactNode;
	children: ReactNode;
}) {
	return (
		<div className="flex items-start justify-between gap-4 px-5 py-3">
			{label}
			<div className="flex min-w-0 flex-col items-end gap-1">
				{children}
			</div>
		</div>
	);
}

const FIELD_ITEM_CLASS =
	"flex flex-row items-start justify-between gap-4 space-y-0 px-5 py-3";

function TextField({
	name,
	label,
	placeholder,
	narrow = false,
}: {
	name: "command" | "args" | "memory";
	label: string;
	placeholder?: string;
	narrow?: boolean;
}) {
	const { control } = useComputeForm();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={FIELD_ITEM_CLASS}>
					<FormLabel className={FIELD_LABEL_CLASS}>{label}</FormLabel>
					<div className="flex min-w-0 flex-col items-end gap-1">
						<FormControl>
							<Input
								className={cn(
									"h-8 text-right font-mono",
									narrow ? "w-28" : "w-56",
								)}
								placeholder={placeholder}
								{...field}
							/>
						</FormControl>
						<FormMessage />
					</div>
				</FormItem>
			)}
		/>
	);
}

function NumberField({
	name,
	label,
}: {
	name:
		| "maxConcurrentActors"
		| "drainGracePeriod"
		| "minScale"
		| "maxScale"
		| "instanceRequestConcurrency";
	label: string;
}) {
	const { control } = useComputeForm();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={FIELD_ITEM_CLASS}>
					<FormLabel className={FIELD_LABEL_CLASS}>{label}</FormLabel>
					<div className="flex min-w-0 flex-col items-end gap-1">
						<FormControl>
							<Input
								type="number"
								className="h-8 w-28 text-right font-mono"
								{...field}
							/>
						</FormControl>
						<FormMessage />
					</div>
				</FormItem>
			)}
		/>
	);
}

function SwitchField({
	name,
	label,
}: {
	name: "drainOnVersionUpgrade";
	label: string;
}) {
	const { control } = useComputeForm();
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className={FIELD_ITEM_CLASS}>
					<FormLabel className={FIELD_LABEL_CLASS}>{label}</FormLabel>
					<FormControl>
						<Switch
							checked={field.value}
							onCheckedChange={field.onChange}
						/>
					</FormControl>
				</FormItem>
			)}
		/>
	);
}

// The CPU slider walks the fractional range [0.08, 1.00] in 0.01 steps on
// the left of the track, then jumps between the whole vCPU values 1, 2, 4,
// and 8 as magnetic stops on the right, mirroring the values `rivet deploy`
// accepts. Positions are integer indices so the fractional zone stays evenly
// spaced while the whole stops get wider spacing than one step each. The
// value can also be typed directly into the input next to the slider; while
// a typed value is out of catalog the thumb snaps to the nearest stop and
// the field shows a validation error.
const CPU_FRACTIONAL_MAX_INDEX = 92; // (1.00 - 0.08) / 0.01
const CPU_WHOLE_STOPS = [
	{ index: 92, cpu: 1 },
	{ index: 122, cpu: 2 },
	{ index: 152, cpu: 4 },
	{ index: 182, cpu: 8 },
];
const CPU_SLIDER_MAX = 182;
const CPU_TICKS = [
	{ label: "0.08", index: 0 },
	{ label: "1", index: 92 },
	{ label: "2", index: 122 },
	{ label: "4", index: 152 },
	{ label: "8", index: 182 },
];

function sliderIndexToCpu(index: number): number {
	if (index <= CPU_FRACTIONAL_MAX_INDEX) {
		return (index + 8) / 100;
	}
	let nearest = CPU_WHOLE_STOPS[0];
	for (const stop of CPU_WHOLE_STOPS) {
		if (Math.abs(stop.index - index) < Math.abs(nearest.index - index)) {
			nearest = stop;
		}
	}
	return nearest.cpu;
}

function cpuToSliderIndex(cpu: number): number {
	if (!Number.isFinite(cpu)) {
		return CPU_FRACTIONAL_MAX_INDEX;
	}
	if (cpu <= 1) {
		return Math.min(
			Math.max(Math.round(cpu * 100) - 8, 0),
			CPU_FRACTIONAL_MAX_INDEX,
		);
	}
	let nearest = CPU_WHOLE_STOPS[0];
	for (const stop of CPU_WHOLE_STOPS) {
		if (Math.abs(stop.cpu - cpu) < Math.abs(nearest.cpu - cpu)) {
			nearest = stop;
		}
	}
	return nearest.index;
}

function CpuField() {
	const { control, getValues, trigger } = useComputeForm();
	// An out-of-catalog CPU value from the server cannot be represented by a
	// slider position (the thumb snaps to the nearest stop), so surface the
	// validation error immediately instead of waiting for the first
	// interaction with the field.
	useEffect(() => {
		if (!isValidCpu(Number(getValues("cpu")))) {
			void trigger("cpu");
		}
	}, [getValues, trigger]);
	return (
		<FormField
			control={control}
			name="cpu"
			render={({ field }) => {
				const cpu = Number(field.value);
				return (
					<FormItem className={FIELD_ITEM_CLASS}>
						<FormLabel className={FIELD_LABEL_CLASS}>CPU</FormLabel>
						<div className="flex w-56 flex-col items-end gap-1.5">
							<div className="flex items-center gap-1.5">
								<FormControl>
									<Input
										className="h-8 w-20 text-right font-mono"
										inputMode="decimal"
										{...field}
									/>
								</FormControl>
								<span className="text-sm text-muted-foreground">
									vCPU
								</span>
							</div>
							<Slider
								min={0}
								max={CPU_SLIDER_MAX}
								step={1}
								value={[cpuToSliderIndex(cpu)]}
								onValueChange={([next]) =>
									field.onChange(sliderIndexToCpu(next))
								}
							/>
							<div className="relative h-4 w-full font-mono text-[10px] text-muted-foreground">
								{CPU_TICKS.map((tick) => (
									<span
										key={tick.label}
										className={cn(
											"absolute top-0",
											tick.index === 0
												? "left-0"
												: tick.index === CPU_SLIDER_MAX
													? "right-0"
													: "-translate-x-1/2",
										)}
										style={
											tick.index !== 0 &&
											tick.index !== CPU_SLIDER_MAX
												? {
														left: `${(tick.index / CPU_SLIDER_MAX) * 100}%`,
													}
												: undefined
										}
									>
										{tick.label}
									</span>
								))}
							</div>
							<FormMessage />
						</div>
					</FormItem>
				);
			}}
		/>
	);
}
