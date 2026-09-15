import { faPlus, Icon } from "@rivet-gg/icons";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { Button, type ButtonProps, WithTooltip } from "@/components";
import { useActorsView } from "./actors-view-context-provider";
import { useDataProvider } from "./data-provider";

export function CreateActorButton({
	label,
	iconOnly,
	renderTooltip,
	...props
}: ButtonProps & {
	label?: string;
	iconOnly?: boolean;
	// Lets a parent supply its own tooltip surface (e.g. a shared
	// CursorTooltipGroup) instead of the default per-button tooltip.
	renderTooltip?: (trigger: ReactNode, content: ReactNode) => ReactNode;
}) {
	const navigate = useNavigate();

	const provider = useDataProvider();

	const { data } = useInfiniteQuery(provider.buildsQueryOptions());

	const { copy } = useActorsView();

	const canCreate = data && data.length > 0;

	if (!provider.features.canCreateActors) {
		return null;
	}

	const onClick = () => {
		navigate({
			to: ".",
			search: (prev) => ({
				...prev,
				modal: "create-actor",
			}),
		});
	};

	const content = iconOnly ? (
		<div>
			<Button
				disabled={!canCreate}
				size="icon-sm"
				variant="ghost"
				onClick={onClick}
				aria-label={label ?? copy.createActor}
				{...props}
			>
				<Icon icon={faPlus} />
			</Button>
		</div>
	) : (
		<div>
			<Button
				disabled={!canCreate}
				size="sm"
				variant="ghost"
				onClick={onClick}
				startIcon={<Icon icon={faPlus} />}
				{...props}
			>
				{label ?? copy.createActor}
			</Button>
		</div>
	);

	// Disabled buttons explain why; icon-only buttons need their label as a
	// tooltip since it isn't visible otherwise.
	const tooltip = !canCreate
		? data && data.length <= 0
			? "Please deploy a build first."
			: copy.createActorUsingForm
		: iconOnly
			? (label ?? copy.createActor)
			: null;

	if (tooltip === null) {
		return content;
	}

	if (renderTooltip) {
		return renderTooltip(content, tooltip);
	}

	return <WithTooltip trigger={content} content={tooltip} />;
}
