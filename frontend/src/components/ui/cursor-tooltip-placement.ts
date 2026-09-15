// Pure placement math for the cursor-following tooltip. Kept separate from
// the component so the fallback order can be unit tested without a DOM.

export const CURSOR_OFFSET = 14;
export const VIEWPORT_PADDING = 8;

export type Point = { x: number; y: number };
export type Size = { width: number; height: number };
export type Viewport = { width: number; height: number };

export type Placement = {
	// Top-left corner of the tooltip box.
	left: number;
	top: number;
	side: "right" | "left" | "bottom";
};

// When the box is larger than the available space (max < min), favor the
// leading edge so the start of the text stays visible.
function clamp(value: number, min: number, max: number) {
	return Math.max(Math.min(value, max), min);
}

// Preferred order: to the right of the cursor, then to the left, then below.
// Right/left keep the tooltip vertically centered on the cursor; below keeps
// it horizontally centered. Whatever side wins, the box is clamped inside the
// viewport padding so it never gets cut off.
export function placeCursorTooltip(
	point: Point,
	size: Size,
	viewport: Viewport,
): Placement {
	const minLeft = VIEWPORT_PADDING;
	const maxLeft = viewport.width - VIEWPORT_PADDING - size.width;
	const minTop = VIEWPORT_PADDING;
	const maxTop = viewport.height - VIEWPORT_PADDING - size.height;

	const centeredTop = clamp(point.y - size.height / 2, minTop, maxTop);

	const rightLeft = point.x + CURSOR_OFFSET;
	if (rightLeft <= maxLeft) {
		return { left: rightLeft, top: centeredTop, side: "right" };
	}

	const leftLeft = point.x - CURSOR_OFFSET - size.width;
	if (leftLeft >= minLeft) {
		return { left: leftLeft, top: centeredTop, side: "left" };
	}

	return {
		left: clamp(point.x - size.width / 2, minLeft, maxLeft),
		top: clamp(point.y + CURSOR_OFFSET, minTop, maxTop),
		side: "bottom",
	};
}
