import type { Rivet } from "@rivetkit/engine-api-full";
import {
	useMutation,
	useSuspenseInfiniteQuery,
	useSuspenseQuery,
} from "@tanstack/react-query";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useFormContext, useWatch } from "react-hook-form";
import { ConfirmableSubmitButton } from "@/app/forms/confirmable-submit-button";

const SERVERLESS_FIELDS = [
	"url",
	"headers",
	"requestLifespan",
	"maxRunners",
	"minRunners",
	"runnersMargin",
	"slotsPerRunner",
	"maxConcurrentActors",
	"drainGracePeriod",
	"autoUpgrade",
] as const;

import * as EditRunnerConfigForm from "@/app/forms/edit-shared-runner-config-form";
import * as EditSingleRunnerConfigForm from "@/app/forms/edit-single-runner-config-form";
import {
	EndpointHealthCheckProvider,
	useEndpointHealthChecksLoading,
	useEndpointHealthChecksValid,
} from "@/app/forms/serverless-endpoint-health";
import {
	Accordion,
	AccordionContent,
	AccordionItem,
	AccordionTrigger,
	Combobox,
	type DialogContentProps,
	Label,
	Separator,
	Switch,
} from "@/components";
import { ActorRegion, useEngineCompatDataProvider } from "@/components/actors";
import { queryClient } from "@/queries/global";

const defaultServerlessConfig: Rivet.RunnerConfigServerless = {
	url: "",
	maxRunners: 100_000,
	requestLifespan: 300,
	slotsPerRunner: 1,
	headers: {},
};

type RuntimeMode = "serverless" | "serverful";

function hasProtocolVersion(
	datacenters: Record<string, Rivet.RunnerConfigResponse>,
): boolean {
	return Object.values(datacenters).some((dc) => dc.protocolVersion != null);
}

function dcHasProtocolVersion(dc: Rivet.RunnerConfigResponse): boolean {
	return dc.protocolVersion != null;
}

function dcMode(
	dc: Rivet.RunnerConfigResponse | undefined,
): RuntimeMode | undefined {
	if (!dc) return undefined;
	if (dc.serverless) return "serverless";
	// `normal` may be present as `{}` for serverful configs.
	if ((dc as { normal?: unknown }).normal !== undefined) return "serverful";
	return undefined;
}

function dcSignatureWithoutMetadata(dc: Rivet.RunnerConfigResponse): string {
	const { metadata: _metadata, ...rest } =
		dc as Rivet.RunnerConfigResponse & {
			metadata?: unknown;
		};
	return JSON.stringify(rest);
}

interface EditRunnerConfigFrameContentProps extends DialogContentProps {
	name: string;
	dc?: string;
}

export default function EditRunnerConfigFrameContent({
	name,
	dc,
	onClose,
}: EditRunnerConfigFrameContentProps) {
	const provider = useEngineCompatDataProvider();

	const { data } = useSuspenseQuery({
		...provider.runnerConfigQueryOptions({ name }),
	});

	const isSharedSettings = useMemo(() => {
		const sigs = Object.values(data.datacenters).map(
			dcSignatureWithoutMetadata,
		);
		return sigs.length === 0 || sigs.every((s) => s === sigs[0]);
	}, [data.datacenters]);

	const [shareAcrossDatacenters, setShareAcrossDatacenters] =
		useState(isSharedSettings);

	return (
		<div className="flex-1 min-h-0 flex flex-col">
			<div className="shrink-0 flex items-center justify-between gap-3 px-5 py-2.5 border-b border-border">
				<Label
					htmlFor="share-across-datacenters"
					className="cursor-pointer text-sm font-normal text-muted-foreground"
				>
					Same settings for all datacenters
				</Label>
				<Switch
					id="share-across-datacenters"
					checked={shareAcrossDatacenters}
					onCheckedChange={setShareAcrossDatacenters}
				/>
			</div>
			<EndpointHealthCheckProvider>
				{shareAcrossDatacenters ? (
					<SharedSettingsForm name={name} onClose={onClose} />
				) : (
					<DatacenterSettingsForm name={name} onClose={onClose} />
				)}
			</EndpointHealthCheckProvider>
		</div>
	);
}

function ResetOnModeChange({ pathPrefix }: { pathPrefix?: string }) {
	const form = useFormContext();
	const modeName = pathPrefix ? `${pathPrefix}.mode` : "mode";
	const mode = useWatch({ name: modeName }) as RuntimeMode | undefined;
	const prevMode = useRef<RuntimeMode | undefined>(undefined);
	useEffect(() => {
		if (prevMode.current === undefined) {
			prevMode.current = mode;
			return;
		}
		if (prevMode.current === mode) return;
		for (const field of SERVERLESS_FIELDS) {
			const path = pathPrefix ? `${pathPrefix}.${field}` : field;
			// biome-ignore lint/suspicious/noExplicitAny: dynamic field path
			form.resetField(path as any);
		}
		prevMode.current = mode;
	}, [mode, form, pathPrefix]);
	return null;
}

function WhenMode({
	name = "mode",
	value,
	children,
}: {
	name?: string;
	value: RuntimeMode;
	children: ReactNode;
}) {
	const mode =
		(useWatch({ name }) as RuntimeMode | undefined) ?? "serverless";
	if (mode !== value) return null;
	return <>{children}</>;
}

function fallbackMetadata(
	datacenters: Record<string, Rivet.RunnerConfigResponse>,
): unknown | undefined {
	for (const dc of Object.values(datacenters)) {
		const meta = (dc as { metadata?: unknown }).metadata;
		if (meta) return meta;
	}
	return undefined;
}

interface ModeSwitch {
	regionId: string;
	from: RuntimeMode;
	to: RuntimeMode;
}

function describeSwitches(switches: ModeSwitch[]): string {
	const grouped = switches.reduce<Record<string, string[]>>((acc, s) => {
		const key = `${labelForMode(s.from)} → ${labelForMode(s.to)}`;
		(acc[key] = acc[key] || []).push(s.regionId);
		return acc;
	}, {});
	return Object.entries(grouped)
		.map(
			([transition, regions]) =>
				`${transition} for ${regions.join(", ")}`,
		)
		.join("; ");
}

function labelForMode(mode: RuntimeMode): string {
	return mode === "serverless" ? "Serverless" : "Runner";
}

function ServerfulModeNotice() {
	return (
		<div className="text-sm text-muted-foreground border rounded-md p-4">
			This is a Runner configuration. Runners connect to Rivet directly
			using the runner SDK. No additional configuration is required here.{" "}
			<a
				href="https://www.rivet.dev/docs/general/runtime-modes/"
				target="_blank"
				rel="noopener noreferrer"
				className="underline"
			>
				Learn more
			</a>
			.
		</div>
	);
}

function SharedSettingsForm({
	onClose,
	name,
}: {
	onClose?: () => void;
	name: string;
}) {
	const provider = useEngineCompatDataProvider();

	const { mutateAsync } = useMutation({
		...provider.upsertRunnerConfigMutationOptions(),
		onSuccess: () => {
			onClose?.();
		},
	});

	const { data } = useSuspenseQuery({
		...provider.runnerConfigQueryOptions({ name }),
	});

	const isNewConfig = hasProtocolVersion(data.datacenters);

	const currentDcConfig =
		Object.values(data.datacenters).find((dc) => !!dc.serverless) ??
		Object.values(data.datacenters).find(
			(dc) => (dc as { normal?: unknown }).normal !== undefined,
		);
	const currentServerless =
		currentDcConfig?.serverless ?? defaultServerlessConfig;

	const detectedMode: RuntimeMode = useMemo(() => {
		const modes = Object.values(data.datacenters)
			.map(dcMode)
			.filter((m): m is RuntimeMode => !!m);
		return modes[0] ?? "serverless";
	}, [data.datacenters]);

	return (
		<EditRunnerConfigForm.Form
			onSubmit={async ({ mode, regions, autoUpgrade, ...values }) => {
				const selectedRegions = regions || {};
				const sharedFallback = fallbackMetadata(data.datacenters);
				const providerConfig: Record<string, Rivet.RunnerConfig> = {};

				for (const [regionId, isSelected] of Object.entries(
					selectedRegions,
				)) {
					if (!isSelected) continue;
					const existing = data.datacenters[regionId] || {};
					const metadata =
						(existing as { metadata?: unknown }).metadata ??
						sharedFallback;
					if (mode === "serverless") {
						const serverless: Rivet.RunnerConfigServerless =
							isNewConfig
								? {
										url: values.url ?? "",
										requestLifespan:
											values.requestLifespan ?? 300,
										headers: Object.fromEntries(
											values.headers || [],
										),
										maxRunners: 0,
										slotsPerRunner: 1,
										maxConcurrentActors:
											values.maxConcurrentActors,
										drainGracePeriod:
											values.drainGracePeriod,
									}
								: {
										url: values.url ?? "",
										requestLifespan:
											values.requestLifespan ?? 300,
										headers: Object.fromEntries(
											values.headers || [],
										),
										maxRunners:
											values.maxRunners ?? 100_000,
										minRunners: values.minRunners ?? 0,
										runnersMargin:
											values.runnersMargin ?? 0,
										slotsPerRunner:
											values.slotsPerRunner ?? 1,
									};
						const { normal: _drop, ...existingRest } =
							existing as Rivet.RunnerConfig & {
								normal?: unknown;
							};
						providerConfig[regionId] = {
							...existingRest,
							...(metadata ? { metadata } : {}),
							serverless,
							...(isNewConfig
								? { drainOnVersionUpgrade: autoUpgrade }
								: {}),
						} as Rivet.RunnerConfig;
					} else {
						const { serverless: _drop, ...existingRest } =
							existing as Rivet.RunnerConfig & {
								serverless?: unknown;
							};
						providerConfig[regionId] = {
							...existingRest,
							...(metadata ? { metadata } : {}),
							normal: {},
						} as Rivet.RunnerConfig;
					}
				}

				await mutateAsync({
					name,
					config: providerConfig,
				});

				await queryClient.invalidateQueries(
					provider.runnerConfigsQueryOptions(),
				);
				await queryClient.refetchQueries(
					provider.runnerConfigQueryOptions({ name }),
				);
				onClose?.();
			}}
			defaultValues={{
				mode: detectedMode,
				url: currentServerless.url,
				requestLifespan: currentServerless.requestLifespan,
				headers: Object.entries(currentServerless.headers || {}).map(
					([key, value]) => [key, value],
				),
				regions: Object.fromEntries(
					Object.keys(data.datacenters).map((dcId) => [
						dcId,
						!!data.datacenters[dcId],
					]),
				),
				...(isNewConfig
					? {
							// eslint-disable-next-line @typescript-eslint/no-explicit-any
							maxConcurrentActors: (currentServerless as any)
								.maxConcurrentActors,
							// eslint-disable-next-line @typescript-eslint/no-explicit-any
							drainGracePeriod: (currentServerless as any)
								.drainGracePeriod,
							autoUpgrade:
								currentDcConfig?.drainOnVersionUpgrade ?? false,
						}
					: {
							maxRunners: currentServerless.maxRunners,
							minRunners: currentServerless.minRunners,
							runnersMargin: currentServerless.runnersMargin,
							slotsPerRunner: currentServerless.slotsPerRunner,
						}),
			}}
		>
			<div className="flex-1 min-h-0 overflow-y-auto px-5 py-5 space-y-6">
				<EditRunnerConfigForm.Mode />
				<ResetOnModeChange />
				<WhenMode value="serverless">
					<div className="space-y-6">
						<div className="space-y-5">
							<EditRunnerConfigForm.Url headersName="headers" />
							{isNewConfig ? (
								<>
									<div className="grid grid-cols-2 gap-3">
										<EditRunnerConfigForm.RequestLifespan />
										<EditRunnerConfigForm.MaxConcurrentActors />
									</div>
									<EditRunnerConfigForm.DrainGracePeriod />
									<EditRunnerConfigForm.AutoUpgrade />
								</>
							) : (
								<>
									<div className="grid grid-cols-2 gap-3">
										<EditRunnerConfigForm.MinRunners />
										<EditRunnerConfigForm.MaxRunners />
									</div>
									<div className="grid grid-cols-2 gap-3">
										<EditRunnerConfigForm.RequestLifespan />
										<EditRunnerConfigForm.SlotsPerRunner />
									</div>
									<EditRunnerConfigForm.RunnersMargin />
								</>
							)}
						</div>
						<Separator />
						<EditRunnerConfigForm.Headers />
					</div>
				</WhenMode>
				<WhenMode value="serverful">
					<ServerfulModeNotice />
				</WhenMode>
				<Separator />
				<EditRunnerConfigForm.Regions />
			</div>
			<div className="shrink-0 border-t border-border bg-card px-5 py-3 flex justify-end gap-2">
				<SharedSettingsSubmit dataDatacenters={data.datacenters} />
			</div>
		</EditRunnerConfigForm.Form>
	);
}

function DatacenterSettingsForm({
	onClose,
	name,
}: {
	onClose?: () => void;
	name: string;
}) {
	const provider = useEngineCompatDataProvider();

	const { data: datacenters } = useSuspenseInfiniteQuery({
		...provider.datacentersQueryOptions(),
		maxPages: Infinity,
	});

	const { mutateAsync } = useMutation({
		...provider.upsertRunnerConfigMutationOptions(),
		onSuccess: () => {
			onClose?.();
		},
	});

	const { data } = useSuspenseQuery({
		...provider.runnerConfigQueryOptions({ name }),
	});

	return (
		<EditSingleRunnerConfigForm.Form
			defaultValues={{
				datacenters: Object.fromEntries(
					Object.values(datacenters).map((dc) => {
						const existing = data.datacenters[dc.name];
						const detectedMode: RuntimeMode =
							dcMode(existing) ?? "serverless";
						return [
							dc.name,
							{
								...(existing?.serverless ||
									defaultServerlessConfig),
								mode: detectedMode,
								enable:
									!!existing?.serverless ||
									(existing as { normal?: unknown })
										?.normal !== undefined,
								headers: Object.entries(
									existing?.serverless?.headers || {},
								).map(([key, value]) => [key, value]),
								autoUpgrade:
									existing?.drainOnVersionUpgrade ?? false,
							},
						];
					}),
				),
			}}
			onSubmit={async (values) => {
				const sharedFallback = fallbackMetadata(data.datacenters);
				const providerConfig: Record<string, Rivet.RunnerConfig> = {};

				for (const [dcId, dcConfig] of Object.entries(
					values.datacenters || {},
				)) {
					if (!dcConfig?.enable) continue;
					const existing = data.datacenters[dcId] || {};
					const metadata =
						(existing as { metadata?: unknown }).metadata ??
						sharedFallback;
					const { enable, mode, headers, autoUpgrade, ...rest } =
						dcConfig;
					if (mode === "serverless" || !mode) {
						const isNew = dcHasProtocolVersion(existing);
						const serverless: Rivet.RunnerConfigServerless = isNew
							? {
									url: rest.url ?? "",
									requestLifespan:
										rest.requestLifespan ?? 300,
									headers: Object.fromEntries(headers || []),
									maxRunners: 0,
									slotsPerRunner: 1,
									maxConcurrentActors:
										rest.maxConcurrentActors,
									drainGracePeriod: rest.drainGracePeriod,
								}
							: {
									url: rest.url ?? "",
									requestLifespan:
										rest.requestLifespan ?? 300,
									headers: Object.fromEntries(headers || []),
									maxRunners: rest.maxRunners ?? 100_000,
									minRunners: rest.minRunners ?? 0,
									runnersMargin: rest.runnersMargin ?? 0,
									slotsPerRunner: rest.slotsPerRunner ?? 1,
								};
						const { normal: _drop, ...existingRest } =
							existing as Rivet.RunnerConfig & {
								normal?: unknown;
							};
						providerConfig[dcId] = {
							...existingRest,
							...(metadata ? { metadata } : {}),
							serverless,
							...(isNew
								? { drainOnVersionUpgrade: autoUpgrade }
								: {}),
						} as Rivet.RunnerConfig;
					} else {
						const { serverless: _drop, ...existingRest } =
							existing as Rivet.RunnerConfig & {
								serverless?: unknown;
							};
						providerConfig[dcId] = {
							...existingRest,
							...(metadata ? { metadata } : {}),
							normal: {},
						} as Rivet.RunnerConfig;
					}
				}

				await mutateAsync({
					name,
					config: providerConfig,
				});

				await queryClient.invalidateQueries(
					provider.runnerConfigsQueryOptions(),
				);
				await queryClient.refetchQueries(
					provider.runnerConfigQueryOptions({ name }),
				);
				onClose?.();
			}}
		>
			<div className="flex-1 min-h-0 overflow-y-auto px-5 py-5 space-y-4">
				<Accordion type="multiple" className="w-full">
					{datacenters.map((dc) => (
						<DatacenterAccordion
							key={dc.name}
							regionId={dc.name}
							name={name}
							data={data!}
						/>
					))}
				</Accordion>

				<EditSingleRunnerConfigForm.Datacenters />
			</div>
			<div className="shrink-0 border-t border-border bg-card px-5 py-3 flex justify-end gap-2">
				<DatacenterSettingsSubmit dataDatacenters={data.datacenters} />
			</div>
		</EditSingleRunnerConfigForm.Form>
	);
}

function DatacenterAccordion({
	regionId,
	name,
	data,
}: {
	regionId: string;
	name: string;
	data: Rivet.RunnerConfigsListResponseRunnerConfigsValue;
}) {
	const isNew = dcHasProtocolVersion(data.datacenters[regionId] || {});

	return (
		<AccordionItem value={regionId} className="-mx-2">
			<AccordionTrigger className="mx-2 py-3 text-sm font-medium">
				<div className="flex items-center gap-3">
					<EditSingleRunnerConfigForm.Enable
						name={`datacenters.${regionId}.enable`}
					/>
					<ActorRegion regionId={regionId} showLabel />
				</div>
			</AccordionTrigger>
			<AccordionContent className="flex flex-col gap-4 text-balance px-2">
				<SelectDatacenterSettingsSource
					name={name}
					currentRegionId={regionId}
				/>

				<EditSingleRunnerConfigForm.Mode
					name={`datacenters.${regionId}.mode`}
				/>
				<ResetOnModeChange pathPrefix={`datacenters.${regionId}`} />

				<WhenMode
					name={`datacenters.${regionId}.mode`}
					value="serverless"
				>
					<EditSingleRunnerConfigForm.Url
						name={`datacenters.${regionId}.url`}
						headersName={`datacenters.${regionId}.headers`}
						enabledName={`datacenters.${regionId}.enable`}
					/>
					{isNew ? (
						<>
							<div className="grid grid-cols-2 gap-2">
								<EditSingleRunnerConfigForm.RequestLifespan
									name={`datacenters.${regionId}.requestLifespan`}
								/>
								<EditSingleRunnerConfigForm.MaxConcurrentActors
									name={`datacenters.${regionId}.maxConcurrentActors`}
								/>
							</div>
							<EditSingleRunnerConfigForm.DrainGracePeriod
								name={`datacenters.${regionId}.drainGracePeriod`}
							/>
							<EditSingleRunnerConfigForm.AutoUpgrade
								name={`datacenters.${regionId}.autoUpgrade`}
							/>
						</>
					) : (
						<>
							<div className="grid grid-cols-2 gap-2">
								<EditSingleRunnerConfigForm.MinRunners
									name={`datacenters.${regionId}.minRunners`}
								/>
								<EditSingleRunnerConfigForm.MaxRunners
									name={`datacenters.${regionId}.maxRunners`}
								/>
							</div>
							<div className="grid grid-cols-2 gap-2">
								<EditSingleRunnerConfigForm.RequestLifespan
									name={`datacenters.${regionId}.requestLifespan`}
								/>
								<EditSingleRunnerConfigForm.SlotsPerRunner
									name={`datacenters.${regionId}.slotsPerRunner`}
								/>
							</div>
							<EditSingleRunnerConfigForm.RunnersMargin
								name={`datacenters.${regionId}.runnersMargin`}
							/>
						</>
					)}
					<EditSingleRunnerConfigForm.Headers
						name={`datacenters.${regionId}.headers`}
					/>
				</WhenMode>
				<WhenMode
					name={`datacenters.${regionId}.mode`}
					value="serverful"
				>
					<ServerfulModeNotice />
				</WhenMode>
			</AccordionContent>
		</AccordionItem>
	);
}

function ConfirmableSaveButton({
	blocked,
	blockedReason,
	computeSwitches,
}: {
	blocked: boolean;
	blockedReason: string | null;
	computeSwitches: () => ModeSwitch[];
}) {
	const getConfirmation = () => {
		const switches = computeSwitches();
		if (switches.length === 0) return null;
		return (
			<>
				Saving will overwrite the existing configuration:{" "}
				{describeSwitches(switches)}.
			</>
		);
	};

	return (
		<ConfirmableSubmitButton
			label="Save"
			blocked={blocked}
			blockedReason={blockedReason}
			getConfirmation={getConfirmation}
		/>
	);
}

function SharedSettingsSubmit({
	dataDatacenters,
}: {
	dataDatacenters: Record<string, Rivet.RunnerConfigResponse>;
}) {
	const valid = useEndpointHealthChecksValid();
	const loading = useEndpointHealthChecksLoading();
	const blocked = !valid || loading;
	const blockedReason = !valid
		? "Endpoint is not reachable. Fix the endpoint to enable saving."
		: loading
			? "Verifying endpoint connection..."
			: null;
	const form = useFormContext();

	const computeSwitches = (): ModeSwitch[] => {
		const values = form.getValues() as {
			mode?: RuntimeMode;
			regions?: Record<string, boolean | undefined>;
		};
		const mode: RuntimeMode = values.mode ?? "serverless";
		const switches: ModeSwitch[] = [];
		for (const [regionId, isSelected] of Object.entries(
			values.regions || {},
		)) {
			if (!isSelected) continue;
			const prev = dcMode(dataDatacenters[regionId]);
			if (prev && prev !== mode) {
				switches.push({ regionId, from: prev, to: mode });
			}
		}
		return switches;
	};

	return (
		<ConfirmableSaveButton
			blocked={blocked}
			blockedReason={blockedReason}
			computeSwitches={computeSwitches}
		/>
	);
}

function DatacenterSettingsSubmit({
	dataDatacenters,
}: {
	dataDatacenters: Record<string, Rivet.RunnerConfigResponse>;
}) {
	const valid = useEndpointHealthChecksValid();
	const loading = useEndpointHealthChecksLoading();
	const blocked = !valid || loading;
	const blockedReason = !valid
		? "One or more endpoints are not reachable. Fix the endpoints to enable saving."
		: loading
			? "Verifying endpoint connection..."
			: null;
	const form = useFormContext();

	const computeSwitches = (): ModeSwitch[] => {
		const values = form.getValues() as {
			datacenters?: Record<
				string,
				{ enable?: boolean; mode?: RuntimeMode } | undefined
			>;
		};
		const switches: ModeSwitch[] = [];
		for (const [dcId, dcConfig] of Object.entries(
			values.datacenters || {},
		)) {
			if (!dcConfig?.enable) continue;
			const submittedMode: RuntimeMode = dcConfig.mode ?? "serverless";
			const prev = dcMode(dataDatacenters[dcId]);
			if (prev && prev !== submittedMode) {
				switches.push({
					regionId: dcId,
					from: prev,
					to: submittedMode,
				});
			}
		}
		return switches;
	};

	return (
		<ConfirmableSaveButton
			blocked={blocked}
			blockedReason={blockedReason}
			computeSwitches={computeSwitches}
		/>
	);
}

function SelectDatacenterSettingsSource({
	currentRegionId,
	name,
}: {
	currentRegionId: string;
	name: string;
}) {
	const form = useFormContext();
	const provider = useEngineCompatDataProvider();

	const { data: datacenters } = useSuspenseInfiniteQuery({
		...provider.datacentersQueryOptions(),
		maxPages: Infinity,
	});

	const { data: runnerConfig } = useSuspenseQuery({
		...provider.runnerConfigQueryOptions({ name }),
	});

	const availableDatacenters = datacenters.filter(
		(dc) =>
			dc.name !== currentRegionId && runnerConfig.datacenters[dc.name],
	);

	if (availableDatacenters.length === 0) {
		return null;
	}

	return (
		<div className="flex gap-2 items-center text-muted-foreground">
			Copy settings from
			<Combobox
				onValueChange={(selectedRegionId) => {
					form.setValue(
						`datacenters.${currentRegionId}`,
						form.getValues(`datacenters.${selectedRegionId}`),
						{ shouldDirty: true, shouldTouch: true },
					);
				}}
				value={undefined}
				placeholder="Select datacenter"
				className="w-auto min-w-[200px]"
				options={availableDatacenters.map((dc) => ({
					value: dc.name,
					label: <ActorRegion regionId={dc.name} showLabel />,
				}))}
			/>
		</div>
	);
}
