import { faVercel, Icon } from "@rivet-gg/icons";
import type { Rivet } from "@rivetkit/engine-api-full";
import {
	useMutation,
	usePrefetchInfiniteQuery,
	useSuspenseInfiniteQuery,
} from "@tanstack/react-query";
import { useMemo } from "react";
import * as ConnectVercelForm from "@/app/forms/connect-quick-vercel-form";
import { type DialogContentProps, ExternalLinkCard, Frame } from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import { successfulBackendSetupEffect } from "@/lib/effects";
import { queryClient } from "@/queries/global";
import { useRivetDsn } from "../env-variables";
import { StepperForm } from "../forms/stepper-form";
import { useEndpoint } from "./connect-manual-serverful-frame";
import {
	buildServerlessConfig,
	ConfigurationAccordion,
} from "./connect-manual-serverless-frame";

const { stepper } = ConnectVercelForm;

interface ConnectQuickVercelFrameContentProps extends DialogContentProps {
	title?: React.ReactNode;
}

export default function ConnectQuickVercelFrameContent({
	onClose,
	title,
}: ConnectQuickVercelFrameContentProps) {
	usePrefetchInfiniteQuery({
		...useEngineCompatDataProvider().datacentersQueryOptions(),
		pages: Infinity,
	});

	const { data: datacenters } = useSuspenseInfiniteQuery(
		useEngineCompatDataProvider().datacentersQueryOptions(),
	);

	return (
		<>
			<Frame.Header>
				<Frame.Title className="gap-2 flex items-center">
					{title ?? (
						<div>
							Add <Icon icon={faVercel} className="ml-0.5" />
							Vercel
						</div>
					)}
				</Frame.Title>
			</Frame.Header>
			<Frame.Content>
				<FormStepper onClose={onClose} datacenters={datacenters} />
			</Frame.Content>
		</>
	);
}

function FormStepper({
	datacenters,
	onClose,
}: {
	onClose?: () => void;
	datacenters: Rivet.Datacenter[];
}) {
	const provider = useEngineCompatDataProvider();
	const { mutateAsync } = useMutation({
		...provider.upsertRunnerConfigMutationOptions(),
		onSuccess: async () => {
			successfulBackendSetupEffect();
			await queryClient.invalidateQueries(
				provider.runnerConfigsQueryOptions(),
			);
			onClose?.();
		},
	});
	return (
		<StepperForm
			{...stepper}
			showAllSteps
			initialStep="deploy"
			mode="all"
			content={{
				"initial-info": () => <StepInitialInfo />,
				deploy: () => <StepDeploy />,
			}}
			onSubmit={async ({ values }) => {
				const payload = await buildServerlessConfig(
					provider,
					{
						...values,
						requestLifespan:
							ConnectVercelForm.VERCEL_REQUEST_LIFESPAN,
					},
					{ provider: "vercel" },
				);

				await mutateAsync({
					name: values.runnerName,
					config: payload,
				});
			}}
			defaultValues={{
				runnerName: "default",
				headers: [],
				drainGracePeriod: ConnectVercelForm.VERCEL_DRAIN_GRACE_PERIOD,
				plan: "hobby",
				datacenters: Object.fromEntries(
					datacenters.map((dc) => [dc.name, true]),
				),
			}}
		/>
	);
}

const useVercelTemplateLink = ({ template }: { template?: string }) => {
	const endpoint = useEndpoint();
	const secretDsn = useRivetDsn({ endpoint, kind: "secret" });
	const publicDsn = useRivetDsn({ endpoint, kind: "publishable" });

	return useMemo(() => {
		const repositoryUrl = `https://github.com/rivet-dev/rivet/tree/main/examples/${template || "chat-room"}`;
		const env = ["RIVET_ENDPOINT", "RIVET_PUBLIC_ENDPOINT"].join(",");
		const projectName = template ?? "rivetkit-vercel";
		const envDefaults = {
			RIVET_ENDPOINT: secretDsn,
			RIVET_PUBLIC_ENDPOINT: publicDsn,
		};
		const url = new URL("https://vercel.com/new/clone");
		url.searchParams.set("repository-url", repositoryUrl);
		url.searchParams.set("env", env);
		url.searchParams.set("project-name", projectName);
		url.searchParams.set("repository-name", projectName);
		url.searchParams.set("envDefaults", JSON.stringify(envDefaults));
		return url.toString();
	}, [secretDsn, publicDsn, template]);
};

export function StepInitialInfo({ template }: { template?: string }) {
	return (
		<div className="space-y-4">
			<p>
				Deploy the {template ? `'${template}'` : "Vercel"} template to
				get started quickly.
			</p>
			<DeployToVercelCard template={template} />
		</div>
	);
}

export const DeployToVercelCard = ({ template }: { template?: string }) => {
	const vercelTemplateLink = useVercelTemplateLink({ template });
	return (
		<ExternalLinkCard
			href={vercelTemplateLink}
			icon={faVercel}
			title={`Deploy ${template ? `'${template}'` : "Template"} to Vercel`}
		/>
	);
};

function StepDeploy() {
	return (
		<>
			<p>
				Deploy your code to Vercel and paste your deployment's endpoint:
			</p>
			<div className="mt-2">
				<ConnectVercelForm.Endpoint />
				<ConfigurationAccordion
					requestLifespan={false}
					prefixFields={<ConnectVercelForm.Plan />}
				/>
				<p className="text-muted-foreground text-sm">
					Need help deploying? See{" "}
					<a
						href="https://vercel.com/docs/deployments"
						target="_blank"
						rel="noreferrer"
						className="underline"
					>
						Vercel's deployment documentation
					</a>
					.
				</p>
			</div>

			<ConnectVercelForm.ConnectionCheck provider="vercel" />
		</>
	);
}
