import type { IconDefinition } from "@fortawesome/fontawesome-svg-core";
import { faBalloon, faBook, faCloudflare, faGamepad } from "@rivet-gg/icons";

const COOKBOOK_ICONS = {
	balloon: faBalloon,
	book: faBook,
	cloudflare: faCloudflare,
	gamepad: faGamepad,
} satisfies Record<string, IconDefinition>;

export function getCookbookIcon(icon?: string): IconDefinition | undefined {
	if (!icon) return undefined;
	return COOKBOOK_ICONS[icon];
}
