import { Icon } from "@rivet-gg/icons";
import { getCookbookIcon } from "./icons";

export interface CookbookCardCover {
	src: string;
	objectPosition?: string;
	transform?: string;
	transformOrigin?: string;
	filter?: string;
	ken?: { x: string; y: string; scale: number };
}

export interface CookbookPageCardData {
	slug: string;
	title: string;
	description: string;
	href: string;
	cover?: CookbookCardCover;
	icon?: string;
	primaryTemplate?: {
		name: string;
		displayName: string;
		noFrontend?: boolean;
	};
	templates: Array<{
		name: string;
		displayName: string;
	}>;
}

// Fallback for any cover that has not tuned its own documentary motion: a
// gentle straight push-in.
const DEFAULT_KEN = { x: "0%", y: "0%", scale: 1.16 };

export function CookbookCard({ page }: { page: CookbookPageCardData }) {
	const drift = page.cover?.ken ?? DEFAULT_KEN;
	const icon = getCookbookIcon(page.icon);

	return (
		<div className="group relative">
			{/* Soft ground beneath the card, mirroring the hero reel's hover glow. */}
			<div className="pointer-events-none absolute -inset-3 rounded-xl bg-ink/5 opacity-0 blur-xl transition-all duration-300 ease-out group-hover:scale-105 group-hover:opacity-100" />
			<a
				href={page.href}
				style={{ boxShadow: "0 10px 28px -12px rgba(27, 25, 22, 0.16)" }}
				className="relative block aspect-[5/7] overflow-hidden bg-ink [container-type:inline-size]"
			>
			{page.cover && (
				<>
					<div
						className="ken-burns absolute inset-0"
						style={
							{
								"--kb-x": drift.x,
								"--kb-y": drift.y,
								"--kb-scale": String(drift.scale),
							} as React.CSSProperties
						}
					>
						<img
							src={page.cover.src}
							alt=""
							loading="lazy"
							className="h-full w-full object-cover"
							style={{
								objectPosition: page.cover.objectPosition,
								transform: page.cover.transform,
								transformOrigin: page.cover.transformOrigin,
								filter: page.cover.filter,
							}}
						/>
					</div>
					<div className="absolute inset-0 bg-[linear-gradient(to_bottom,rgba(0,0,0,0.8),rgba(0,0,0,0.3)_32%,transparent_58%)]" />
				</>
			)}
			{!page.cover && icon && (
				<div className="absolute inset-0 flex items-center justify-center text-cream/25">
					<Icon icon={icon} className="h-[22cqw] w-[22cqw]" />
				</div>
			)}
			{/* Title sizes use container-query units so the lockup scales with the card. */}
			<h3
				className="absolute inset-x-0 top-0 px-[8cqw] pt-[12cqw] text-center text-[5.6cqw] font-semibold uppercase leading-[1.55] tracking-[0.2em] text-zinc-50 [text-wrap:balance]"
				style={{ textShadow: "0 2px 16px rgba(0,0,0,0.6)" }}
			>
				{page.title}
			</h3>
			</a>
		</div>
	);
}
