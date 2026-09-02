import { faTrash, Icon } from "@rivet-gg/icons";
import type { Provider } from "@rivetkit/shared-data";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useFieldArray, useFormContext } from "react-hook-form";
import z from "zod";
import {
	endpointSchema,
	ServerlessConnectionCheck,
} from "@/app/serverless-connection-check";
import {
	Button,
	FormControl,
	FormDescription,
	FormField,
	FormFieldContext,
	FormItem,
	FormLabel,
	FormMessage,
	Input,
	Label,
} from "@/components";
import { ActorRegion, useEngineCompatDataProvider } from "@/components/actors";
import { RegionSelect } from "@/components/actors/region-select";
import { IconPicker, IconRenderer } from "@/components/ui/icon-picker";
import { defineStepper } from "@/components/ui/stepper";

export { endpointSchema };

export const configurationSchema = z.object({
	runnerName: z.string().min(1, "Runner name is required"),
	datacenters: z
		.record(z.string(), z.boolean())
		.refine(
			(data) => Object.values(data).some(Boolean),
			"At least one datacenter must be selected",
		),
	headers: z.array(z.tuple([z.string(), z.string()])).default([]),
	requestLifespan: z.coerce.number().min(0, "Must be 0 or greater"),
	drainGracePeriod: z.coerce.number().min(0).optional().default(0),
	customName: z.string().trim().max(32, "Name is too long").optional(),
	customIcon: z.string().optional(),
	// Deprecated fields — only used when submitting to an old runner config (no protocolVersion).
	slotsPerRunner: z.coerce.number().min(1).optional(),
	maxRunners: z.coerce.number().min(1).optional(),
	minRunners: z.coerce.number().min(0).optional(),
	runnerMargin: z.coerce.number().min(0).optional(),
});

export function validateDrainGracePeriod(
	data: { requestLifespan?: number; drainGracePeriod?: number },
	ctx: z.RefinementCtx,
) {
	if (
		data.drainGracePeriod !== undefined &&
		data.requestLifespan !== undefined &&
		data.drainGracePeriod >= data.requestLifespan
	) {
		const message =
			"Drain grace period must be less than the request lifespan.";
		ctx.addIssue({
			path: ["drainGracePeriod"],
			code: z.ZodIssueCode.custom,
			message,
		});
		ctx.addIssue({
			path: ["requestLifespan"],
			code: z.ZodIssueCode.custom,
			message,
		});
	}
}

export const configurationStepSchema = configurationSchema.superRefine(
	validateDrainGracePeriod,
);

export const deploymentSchema = z.object({
	success: z.boolean().refine((val) => val, "Connection failed"),
	endpoint: endpointSchema,
});

export const stepper = defineStepper(
	{
		id: "step-1",
		title: "Configure",
		assist: false,
		next: "Next",
		schema: configurationStepSchema,
	},
	{
		id: "step-2",
		title: "Edit vercel.json",
		assist: false,
		next: "Next",
		schema: z.object({}),
	},
	{
		id: "step-3",
		title: "Deploy to Vercel",
		assist: true,
		next: "Done",
		schema: deploymentSchema,
	},
);

export const RunnerName = function RunnerName() {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="runnerName"
			render={({ field }) => (
				<FormItem>
					<FormLabel className="col-span-1">Runner Name</FormLabel>
					<FormControl className="row-start-2">
						<Input type="text" {...field} />
					</FormControl>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const CustomBranding = function CustomBranding() {
	const { control, watch } = useFormContext();
	const icon = watch("customIcon");
	const name = watch("customName");
	return (
		<div className="space-y-2">
			<FormLabel asChild>
				<p>Display</p>
			</FormLabel>
			<FormDescription>
				Pick an icon and label for this provider. Shown in the provider
				list.
			</FormDescription>
			<div className="flex items-start gap-2">
				<FormField
					control={control}
					name="customIcon"
					render={({ field }) => (
						<FormItem>
							<FormControl>
								<IconPicker
									value={field.value || null}
									onChange={(v) => field.onChange(v ?? "")}
								/>
							</FormControl>
						</FormItem>
					)}
				/>
				<FormField
					control={control}
					name="customName"
					render={({ field }) => (
						<FormItem className="flex-1">
							<FormControl>
								<Input
									type="text"
									placeholder="My custom cluster"
									maxLength={32}
									{...field}
									value={field.value ?? ""}
								/>
							</FormControl>
							<FormMessage />
						</FormItem>
					)}
				/>
			</div>
			{name || icon ? (
				<div className="flex items-center gap-2 text-xs text-muted-foreground">
					Preview:
					<span className="inline-flex items-center gap-1.5 rounded bg-muted px-2 py-1 text-foreground">
						{icon ? (
							<IconRenderer name={icon} className="size-3.5" />
						) : null}
						{name || "Custom"}
					</span>
				</div>
			) : null}
		</div>
	);
};

export const Datacenters = function Datacenter() {
	const { control, watch } = useFormContext();
	const { data: datacenterCount } = useInfiniteQuery({
		...useEngineCompatDataProvider().datacentersQueryOptions(),
		select: (data) =>
			data.pages.reduce(
				(prev, current) => prev + current.datacenters.length,
				0,
			),
	});

	return (
		<div className="space-y-2">
			<FormField
				control={control}
				name="datacenters"
				render={({ field }) => (
					<>
						<FormLabel>Datacenters</FormLabel>
						<FormDescription className="mb-4">
							{watch("provider") === "rivet"
								? "Select the region(s) to enable for Rivet. Rivet's edge network handles multi-region routing automatically."
								: "Select the region(s) that your backend is running in. Rivet's edge network handles multi-region routing automatically."}
						</FormDescription>
						<RegionSelect
							showAuto={false}
							value={Object.keys(field.value || {}).filter(
								(key) => field.value[key],
							)}
							renderCurrentOptions={(options) => {
								if (options.length === datacenterCount) {
									return <span>Global</span>;
								}
								return options.map((option) => (
									<span key={option.value} className="mr-2">
										<ActorRegion
											regionId={option.value}
											showLabel
										/>
									</span>
								));
							}}
							onValueChange={(value) => {
								field.onChange(
									value.reduce(
										(acc, key) => {
											acc[key] = true;
											return acc;
										},
										{} as Record<string, boolean>,
									),
								);
							}}
							multiple
						/>
						<FormMessage />
					</>
				)}
			/>
		</div>
	);
};

export const RequestLifespan = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="requestLifespan"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">
						Request Lifespan (seconds)
					</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						The maximum duration (in seconds) that a request can run
						before being terminated.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const MinRunners = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="minRunners"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">Min Runners</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value ?? ""}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						The minimum number of runners to keep running.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const MaxRunners = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="maxRunners"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">Max Runners</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value ?? ""}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						The maximum number of runners that can be created to
						handle load.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const SlotsPerRunner = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="slotsPerRunner"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">
						Slots Per Runner
					</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value ?? ""}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						The number of concurrent slots each runner can handle.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const RunnerMargin = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="runnerMargin"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">Runner Margin</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value ?? ""}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						The number of extra runners to keep running to handle
						sudden spikes in load.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const DrainGracePeriod = ({ className }: { className?: string }) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="drainGracePeriod"
			render={({ field }) => (
				<FormItem className={className}>
					<FormLabel className="col-span-1">
						Drain Grace Period (s)
					</FormLabel>
					<FormControl className="row-start-2">
						<Input
							type="number"
							{...field}
							value={field.value ?? ""}
							min={0}
						/>
					</FormControl>
					<FormDescription className="col-span-1">
						Time to wait for actors to finish before forcefully
						stopping.
					</FormDescription>
					<FormMessage className="col-span-1" />
				</FormItem>
			)}
		/>
	);
};

export const Headers = function Headers() {
	const { control, setValue, watch } = useFormContext();
	const { fields, append, remove } = useFieldArray({
		name: "headers",
		control,
	});

	return (
		<div className="space-y-2">
			<FormLabel asChild>
				<p>Custom Headers</p>
			</FormLabel>
			<FormDescription>
				Custom headers to add to each request to the runner. Useful for
				providing authentication or other information.
			</FormDescription>
			<div className="grid grid-cols-[1fr,1fr,auto] grid-rows-[repeat(3,auto)] items-start gap-2 empty:hidden">
				{fields.length > 0 ? (
					<>
						<Label asChild>
							<p>Name</p>
						</Label>
						<Label asChild>
							<p>Value</p>
						</Label>
						<p></p>
					</>
				) : null}
				{fields.map((field, index) => (
					<div
						key={field.id}
						className="grid grid-cols-subgrid grid-rows-
col-span-full flex-1"
					>
						<FormFieldContext.Provider
							value={{ name: `headers.${index}.0` }}
						>
							<FormItem
								flex="1"
								className="grid grid-cols-subgrid grid-rows-subgrid row-span-full"
							>
								<FormLabel aria-hidden hidden>
									Key
								</FormLabel>
								<FormControl>
									<Input
										placeholder="Enter a value"
										className="w-full"
										value={watch(`headers.${index}.0`)}
										onChange={(e) => {
											setValue(
												`headers.${index}.0`,
												e.target.value,
												{
													shouldDirty: true,
													shouldTouch: true,
													shouldValidate: true,
												},
											);
										}}
									/>
								</FormControl>
								<FormMessage />
							</FormItem>
						</FormFieldContext.Provider>

						<FormFieldContext.Provider
							value={{ name: `headers.${index}.1` }}
						>
							<FormItem
								flex="1"
								className="grid grid-cols-subgrid grid-rows-subgrid row-span-full"
							>
								<FormLabel aria-hidden hidden>
									Value
								</FormLabel>
								<FormControl>
									<Input
										placeholder="Enter a value"
										className="w-full"
										value={watch(`headers.${index}.1`)}
										onChange={(e) => {
											setValue(
												`headers.${index}.1`,
												e.target.value,
												{
													shouldDirty: true,
													shouldTouch: true,
													shouldValidate: true,
												},
											);
										}}
									/>
								</FormControl>
								<FormMessage />
							</FormItem>
						</FormFieldContext.Provider>
						<Button
							size="icon"
							className="self-end row-start-1"
							variant="secondary"
							type="button"
							onClick={() => remove(index)}
						>
							<Icon icon={faTrash} />
						</Button>
					</div>
				))}
			</div>
			<Button
				className="justify-self-start"
				variant="secondary"
				size="sm"
				type="button"
				onClick={() => append([["", ""]])}
			>
				Add a header
			</Button>
		</div>
	);
};

export const Endpoint = ({
	className,
	placeholder,
}: {
	className?: string;
	placeholder?: string;
}) => {
	const { control } = useFormContext();
	return (
		<FormField
			control={control}
			name="endpoint"
			render={({ field }) => {
				return (
					<FormItem className={className}>
						<FormLabel className="col-span-1">Endpoint</FormLabel>
						<FormDescription className="col-span-1">
							Enter the publicly accessible URL of your deployed
							backend.
						</FormDescription>
						<FormControl className="row-start-2">
							<Input
								type="text"
								placeholder={
									placeholder ||
									"https://my-rivet-app.vercel.app/api/rivet"
								}
								{...field}
								value={field.value || ""}
								onChange={(e) => {
									const value = e.target.value;
									field.onChange(value);
								}}
								onPaste={(e) => {
									const pastedText = e.clipboardData
										.getData("text")
										.trim();
									// Auto-add https:// if pasted URL doesn't have a protocol
									if (
										pastedText &&
										!pastedText.match(/^https?:\/\//i)
									) {
										e.preventDefault();
										field.onChange(`https://${pastedText}`);
									}
								}}
							/>
						</FormControl>
						<FormMessage className="col-span-1" />
					</FormItem>
				);
			}}
		/>
	);
};

export function ConnectionCheck({ provider }: { provider: Provider }) {
	return (
		<ServerlessConnectionCheck provider={provider} pollIntervalMs={3_000} />
	);
}
