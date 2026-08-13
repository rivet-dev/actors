import {
	faAws,
	faCloudflare,
	faDocker,
	faGoogleCloud,
	faKubernetes,
	faRailway,
	faRocket,
	faServer,
	faSupabase,
	faVercel,
} from "@rivet-gg/icons";

export type Provider =
	| "docker-compose"
	| "kubernetes"
	| "vm"
	| "aws-ecs"
	| "railway"
	| "custom"
	| "vercel"
	| "cloudflare-workers"
	| "gcp-cloud-run"
	| "aws-lambda"
	| "supabase-functions";

/**
 * The two things a platform can host. A worker runs the user's code with the
 * Rivet SDK; the control plane routes, schedules, and persists.
 */
export type DeployRole = "worker" | "control-plane";

/**
 * Three-valued so the template can render a caveat automatically. A boolean
 * cannot express "yes, but": Lambda hosting long-lived actors works and is
 * usually a bad idea, and Cloud Run cold-starts without min-instances.
 */
export type DeploySupport = "recommended" | "supported" | "unsupported";

export interface DeployOption {
	displayName: string;
	name: Provider;
	shortTitle?: string;
	/**
	 * Guide slug, not a URL. Deploy guides are per-product
	 * (`/{product}/self-host/{role}/{slug}`), so consumers build the href.
	 */
	slug: string;
	description: string;
	icon?: any;
	badge?: string;
	/** If true, this platform should NOT be shown for generic deploy guides for Node/Bun-specific platforms. */
	specializedPlatform?: boolean;
	/**
	 * Which halves this platform can host. The control plane is stateful, so it
	 * needs persistent storage and stable network identity; every serverless
	 * platform is therefore worker-only. That is one property, not a
	 * per-platform exception.
	 */
	support: Record<DeployRole, DeploySupport>;
}

export const deployOptions: DeployOption[] = [
	{
		displayName: "Docker Compose",
		name: "docker-compose",
		shortTitle: "Compose",
		slug: "docker-compose",
		description: "Run Rivet with Docker Compose on a single host",
		icon: faDocker as any,
		support: { worker: "supported", "control-plane": "recommended" },
	},
	{
		displayName: "Kubernetes",
		name: "kubernetes",
		slug: "kubernetes",
		description: "Deploy to any Kubernetes cluster with container images",
		icon: faKubernetes as any,
		support: { worker: "supported", "control-plane": "recommended" },
	},
	{
		displayName: "Virtual Machine",
		name: "vm",
		shortTitle: "VM",
		slug: "vm",
		description:
			"Run on virtual machines or bare metal servers with full control",
		icon: faServer as any,
		support: { worker: "supported", "control-plane": "supported" },
	},
	{
		displayName: "AWS ECS",
		shortTitle: "AWS",
		name: "aws-ecs",
		slug: "aws-ecs",
		description:
			"Run containerized workloads on Amazon Elastic Container Service",
		icon: faAws as any,
		support: { worker: "supported", "control-plane": "supported" },
	},
	{
		displayName: "Railway",
		name: "railway",
		slug: "railway",
		description: "Deploy containers to Railway's managed infrastructure",
		icon: faRailway as any,
		support: { worker: "supported", "control-plane": "supported" },
	},
	{
		displayName: "Custom Platform",
		name: "custom",
		shortTitle: "Custom",
		slug: "custom",
		description:
			"Integrate Rivet with any other hosting platform of your choice",
		icon: faRocket as any,
		support: { worker: "supported", "control-plane": "supported" },
	},
	{
		displayName: "Vercel",
		name: "vercel",
		slug: "vercel",
		description: "Deploy Next.js + Rivet apps to Vercel's edge network",
		icon: faVercel as any,
		support: { worker: "supported", "control-plane": "unsupported" },
	},
	{
		displayName: "Cloudflare Workers",
		shortTitle: "Cloudflare",
		name: "cloudflare-workers",
		slug: "cloudflare",
		description: "Run Rivet on Cloudflare Workers with the WebAssembly runtime",
		icon: faCloudflare as any,
		specializedPlatform: true,
		support: { worker: "supported", "control-plane": "unsupported" },
	},
	{
		displayName: "Google Cloud Run",
		shortTitle: "GCP",
		name: "gcp-cloud-run",
		slug: "gcp-cloud-run",
		description: "Deploy containers to Google Cloud Run for auto-scaling",
		icon: faGoogleCloud,
		support: { worker: "supported", "control-plane": "unsupported" },
	},
	{
		displayName: "AWS Lambda",
		shortTitle: "Lambda",
		name: "aws-lambda",
		slug: "aws-lambda",
		description: "Run Rivet workers on AWS Lambda functions",
		icon: faAws as any,
		support: { worker: "supported", "control-plane": "unsupported" },
	},
	{
		displayName: "Supabase Functions",
		shortTitle: "Supabase",
		name: "supabase-functions",
		slug: "supabase",
		description:
			"Run Rivet on Supabase Edge Functions with the WebAssembly runtime",
		icon: faSupabase,
		specializedPlatform: true,
		support: { worker: "supported", "control-plane": "unsupported" },
	},
];

export function deployOptionsForRole(role: DeployRole): DeployOption[] {
	return deployOptions.filter(
		(option) => option.support[role] !== "unsupported",
	);
}
