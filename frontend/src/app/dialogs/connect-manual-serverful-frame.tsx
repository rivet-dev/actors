import type { Rivet } from "@rivetkit/engine-api-full";
import { deployOptions, type Provider } from "@rivetkit/shared-data";
import {
	useMutation,
	usePrefetchInfiniteQuery,
	useQuery,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import { useMemo, useRef } from "react";
import { useWatch } from "react-hook-form";
import z from "zod";
import * as ConnectServerfulForm from "@/app/forms/connect-manual-serverful-form";
import * as ConnectServerlessForm from "@/app/forms/connect-manual-serverless-form";
import type { DialogContentProps } from "@/components";
import { ActorRegion, useEngineCompatDataProvider } from "@/components/actors";
import { defineStepper } from "@/components/ui/stepper";
import { successfulBackendSetupEffect } from "@/lib/effects";
import { engineEnv } from "@/lib/env";
import { queryClient } from "@/queries/global";
import { EnvVariables } from "../env-variables";
import { type StepConfirm, StepperForm } from "../forms/stepper-form";

type FormValues = {
	runnerName: string;
	datacenter: string;
	customName?: string;
	customIcon?: string;
};

function useStepperConfig() {
	const dataProvider = useEngineCompatDataProvider();
	const dataProviderRef = useRef(dataProvider);
	dataProviderRef.current = dataProvider;

	return useMemo(() => {
		const confirmStep2: StepConfirm<FormValues> = async ({
			runnerName,
			datacenter,
		}) => {
			if (!runnerName) return null;
			const data = await queryClient.fetchQuery(
				dataProviderRef.current.runnerConfigQueryOptions({
					name: runnerName,
					safe: true,
				}),
			);
			const existingDatacenters = data
				? Object.keys(data.datacenters || {})
				: [];
			if (existingDatacenters.length === 0) return null;
			const willReplaceDatacenter =
				!!datacenter && existingDatacenters.includes(datacenter);
			return (
				<>
					A runner config named{" "}
					<code className="rounded bg-muted px-1 py-0.5 text-foreground">
						{runnerName}
					</code>{" "}
					already exists
					{willReplaceDatacenter ? (
						<>
							. Submitting will overwrite its existing
							configuration for{" "}
							<DatacenterLabel regionId={datacenter} />.
						</>
					) : (
						<>
							. Submitting will add{" "}
							<DatacenterLabel regionId={datacenter} /> to it.
						</>
					)}
				</>
			);
		};

		return defineStepper(
			{
				id: "step-1",
				title: "Configure",
				assist: false,
				schema: z.object({
					runnerName: z.string().min(1, "Runner name is required"),
					datacenter: z.string().min(1, "Please select a region"),
					customName: z
						.string()
						.trim()
						.max(32, "Name is too long")
						.optional(),
					customIcon: z.string().optional(),
				}),
			},
			{
				id: "step-2",
				title: "Deploy",
				assist: false,
				schema: z.object({}),
				next: "Add",
				confirm: confirmStep2,
			},
		);
	}, []);
}

interface ConnectManualServerfulFrameContentProps extends DialogContentProps {
	provider: Provider;
	footer?: React.ReactNode;
}

export default function ConnectManualServerfulFrameContent({
	onClose,
	provider,
	footer,
}: ConnectManualServerfulFrameContentProps) {
	usePrefetchInfiniteQuery({
		...useEngineCompatDataProvider().datacentersQueryOptions(),
		pages: Infinity,
	});
	const { data } = useSuspenseInfiniteQuery(
		useEngineCompatDataProvider().datacentersQueryOptions(),
	);

	const prefferedRegionForRailway =
		data.find((region) => region.name.toLowerCase().includes("us-west"))
			?.name ||
		data.find((region) => region.name.toLowerCase().includes("us-east"))
			?.name ||
		data.find((region) => region.name.toLowerCase().includes("ore"))
			?.name ||
		data[0]?.name ||
		"";

	return (
		<FormStepper
			footer={footer}
			onClose={onClose}
			provider={provider}
			defaultDatacenter={prefferedRegionForRailway}
		/>
	);
}

function FormStepper({
	onClose,
	defaultDatacenter,
	provider,
	footer,
}: {
	onClose?: () => void;
	provider: Provider;
	defaultDatacenter: string;
	footer?: React.ReactNode;
}) {
	const dataProvider = useEngineCompatDataProvider();
	const stepper = useStepperConfig();

	const { mutateAsync } = useMutation({
		...dataProvider.upsertRunnerConfigMutationOptions(),
		onSuccess: async () => {
			successfulBackendSetupEffect();

			await queryClient.invalidateQueries(
				dataProvider.runnerConfigsQueryOptions(),
			);
			onClose?.();
		},
	});
	return (
		<StepperForm
			{...stepper}
			controls={footer}
			onSubmit={async ({ values }) => {
				const runnerConfig = await queryClient.fetchQuery(
					dataProvider.runnerConfigQueryOptions({
						name: values.runnerName,
						safe: true,
					}),
				);
				const existing: Record<string, Rivet.RunnerConfig> =
					runnerConfig?.datacenters || {};

				const isCustom =
					provider === "custom" || provider === "custom-platform";
				const customName = isCustom
					? values.customName?.trim() || undefined
					: undefined;
				const customIcon = isCustom
					? values.customIcon || undefined
					: undefined;
				await mutateAsync({
					name: values.runnerName,
					config: {
						...existing,
						[values.datacenter]: {
							normal: {},
							metadata: {
								provider,
								...(customName ? { customName } : {}),
								...(customIcon ? { customIcon } : {}),
							},
						},
					},
				});
			}}
			defaultValues={{
				runnerName: "default",
				datacenter: defaultDatacenter,
			}}
			content={{
				"step-1": () => <Step1 provider={provider} />,
				"step-2": () => <Step2 provider={provider} />,
			}}
		/>
	);
}

function Step1({ provider }: { provider: Provider }) {
	const isCustom = provider === "custom" || provider === "custom-platform";
	return (
		<>
			<div>
				We're going to help you deploy a RivetKit project to your cloud
				provider of choice.
			</div>
			<ConnectServerfulForm.RunnerName />
			{isCustom ? <ConnectServerlessForm.CustomBranding /> : null}
			<ConnectServerfulForm.Datacenter />
		</>
	);
}

function Step2({ provider }: { provider: Provider }) {
	const providerOptions = deployOptions.find(
		(option) => option.name === provider,
	);
	const runnerName = useWatch({ name: "runnerName" });

	return (
		<>
			<p>
				<a
					href={`https://www.rivet.dev/${providerOptions?.href || "docs/getting-started"}`}
					className="underline"
					target="_blank"
					rel="noopener"
				>
					Follow the integration guide here
				</a>
				, and make sure to set the following environment variables:
			</p>

			<EnvVariables
				endpoint={useEndpoint()}
				runnerName={runnerName}
				showPublicEndpoint={false}
			/>
		</>
	);
}

function DatacenterLabel({ regionId }: { regionId?: string }) {
	return (
		<span className="inline-flex items-center rounded bg-muted px-1.5 py-0.5 text-foreground align-middle">
			<ActorRegion regionId={regionId} showLabel />
		</span>
	);
}

export const useEndpoint = () => {
	const datacenter = useWatch({ name: "datacenter" });

	const { data } = useQuery(
		useEngineCompatDataProvider().datacenterQueryOptions(
			datacenter || "auto",
		),
	);

	return data?.url || engineEnv().VITE_APP_API_URL;
};
