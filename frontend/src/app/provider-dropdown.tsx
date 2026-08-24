import {
	faAws,
	faGoogleCloud,
	faHetznerH,
	faRailway,
	faRivet,
	faServer,
	faVercel,
	Icon,
} from "@rivet-gg/icons";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components";
import { useEngineCompatDataProvider } from "@/components/actors";
import { deriveProviderFromMetadata } from "@/lib/data";
import { features } from "@/lib/features";

export function ProviderDropdown({ children }: { children: React.ReactNode }) {
	const navigate = useNavigate();

	const externalClouds = (
		<>
			<DropdownMenuItem
				className="relative"
				indicator={<Icon icon={faVercel} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-vercel" }),
					})
				}
			>
				Vercel
			</DropdownMenuItem>
			<DropdownMenuItem
				indicator={<Icon icon={faRailway} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-railway" }),
					})
				}
			>
				Railway
			</DropdownMenuItem>
			<DropdownMenuItem
				indicator={<Icon icon={faAws} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-aws" }),
					})
				}
			>
				AWS ECS
			</DropdownMenuItem>
			<DropdownMenuItem
				indicator={<Icon icon={faGoogleCloud} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-gcp" }),
					})
				}
			>
				Google Cloud Run
			</DropdownMenuItem>
			<DropdownMenuItem
				indicator={<Icon icon={faHetznerH} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-hetzner" }),
					})
				}
			>
				Hetzner
			</DropdownMenuItem>
			<DropdownMenuItem
				indicator={<Icon icon={faServer} />}
				onSelect={() =>
					navigate({
						to: ".",
						search: (s) => ({ ...s, modal: "connect-custom" }),
					})
				}
			>
				Custom
			</DropdownMenuItem>
		</>
	);
	return (
		<DropdownMenu>
			<DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
			<DropdownMenuContent className="min-w-[--radix-popper-anchor-width]">
				{features.compute ? <RivetCloudDropdownMenuItem /> : null}
				{externalClouds}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

function RivetCloudDropdownMenuItem() {
	const navigate = useNavigate();

	const { data: config } = useInfiniteQuery({
		...useEngineCompatDataProvider().runnerConfigsQueryOptions(),
		refetchInterval: 5000,
		maxPages: Infinity,
		select: (data) =>
			data.pages.flatMap((page) =>
				Object.values(page.runnerConfigs).filter(
					(config) =>
						Object.values(config.datacenters).find(
							(dc) =>
								deriveProviderFromMetadata(dc.metadata) ===
								"rivet",
						) !== undefined,
				),
			).length > 0,
	});

	return (
		<DropdownMenuItem
			className="relative"
			indicator={<Icon icon={faRivet} />}
			disabled={!!config}
			onSelect={() =>
				navigate({
					to: ".",
					search: (s) => ({ ...s, modal: "connect-rivet" }),
				})
			}
		>
			Rivet Compute {config ? "(Connected)" : ""}
		</DropdownMenuItem>
	);
}
