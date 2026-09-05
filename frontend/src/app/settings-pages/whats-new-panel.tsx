import { faArrowUpRight, Icon } from "@rivet-gg/icons";
import { useQuery } from "@tanstack/react-query";
import {
	Avatar,
	AvatarFallback,
	AvatarImage,
	Picture,
	PictureFallback,
	PictureImage,
	Skeleton,
} from "@/components";
import { changelogQueryOptions } from "@/queries/global";
import type { ChangelogItem } from "@/queries/types";

export function WhatsNewPanel() {
	const { data, isLoading, isError } = useQuery(changelogQueryOptions());

	if (isLoading) {
		return (
			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
				<EntrySkeleton />
				<EntrySkeleton />
				<EntrySkeleton />
				<EntrySkeleton />
			</div>
		);
	}

	if (isError || !data) {
		return (
			<div className="flex h-64 items-center justify-center rounded-lg border border-dashed border-border">
				<p className="text-sm text-muted-foreground">
					Couldn't load the changelog. See{" "}
					<a
						href="https://www.rivet.dev/changelog"
						target="_blank"
						rel="noreferrer"
						className="underline hover:text-foreground"
					>
						rivet.dev/changelog
					</a>
					.
				</p>
			</div>
		);
	}

	if (data.length === 0) {
		return (
			<div className="flex h-64 items-center justify-center rounded-lg border border-dashed border-border">
				<p className="text-sm text-muted-foreground">No updates yet.</p>
			</div>
		);
	}

	return (
		<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
			{data.map((entry) => (
				<Entry key={entry.slug} {...entry} />
			))}
		</div>
	);
}

function Entry({
	published,
	images,
	title,
	description,
	slug,
	authors,
}: ChangelogItem) {
	const image = images[0];
	const author = authors[0];
	return (
		<a
			href={`https://www.rivet.dev/changelog/${slug}`}
			target="_blank"
			rel="noreferrer"
			className="group block rounded-lg border border-foreground/10 bg-card overflow-hidden transition-colors hover:border-foreground/20"
		>
			{image ? (
				<Picture className="block aspect-[2/1] w-full overflow-hidden border-b border-foreground/10">
					<PictureFallback>
						<Skeleton className="size-full" />
					</PictureFallback>
					<PictureImage
						className="size-full object-cover"
						src={image.url}
						width={image.width}
						height={image.height}
						alt={title}
					/>
				</Picture>
			) : null}
			<div className="px-5 py-4">
				<div className="flex items-center gap-2 text-xs text-muted-foreground">
					<time dateTime={published}>
						{new Date(published).toLocaleDateString(undefined, {
							year: "numeric",
							month: "short",
							day: "numeric",
						})}
					</time>
				</div>
				<h3 className="mt-1 text-sm font-semibold text-foreground group-hover:underline">
					{title}
				</h3>
				<p className="mt-1 text-xs text-muted-foreground line-clamp-3">
					{description}
				</p>
				<div className="mt-3 flex items-center justify-between">
					{author ? (
						<div className="flex items-center gap-2">
							<Avatar className="size-6">
								<AvatarFallback>
									{author.name[0]}
								</AvatarFallback>
								<AvatarImage
									src={author.avatar.url}
									alt={author.name}
								/>
							</Avatar>
							<div className="leading-tight">
								<p className="text-xs font-medium text-foreground">
									{author.name}
								</p>
								<p className="text-[10px] text-muted-foreground">
									{author.role}
								</p>
							</div>
						</div>
					) : (
						<span />
					)}
					<span className="inline-flex items-center gap-1 text-xs text-muted-foreground group-hover:text-foreground">
						Read more
						<Icon icon={faArrowUpRight} className="size-3" />
					</span>
				</div>
			</div>
		</a>
	);
}

function EntrySkeleton() {
	return (
		<div className="rounded-lg border border-foreground/10 bg-card overflow-hidden">
			<Skeleton className="aspect-[2/1] w-full" />
			<div className="px-5 py-4 space-y-2">
				<Skeleton className="h-3 w-20" />
				<Skeleton className="h-4 w-3/4" />
				<Skeleton className="h-3 w-full" />
				<Skeleton className="h-3 w-5/6" />
			</div>
		</div>
	);
}
