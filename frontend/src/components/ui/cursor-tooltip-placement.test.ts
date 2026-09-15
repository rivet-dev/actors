import { describe, expect, it } from "vitest";
import {
	CURSOR_OFFSET,
	placeCursorTooltip,
	VIEWPORT_PADDING,
} from "./cursor-tooltip-placement";

const viewport = { width: 1000, height: 600 };
const size = { width: 120, height: 30 };

describe("placeCursorTooltip", () => {
	it("prefers the right of the cursor, vertically centered", () => {
		const p = placeCursorTooltip({ x: 300, y: 200 }, size, viewport);
		expect(p).toEqual({
			side: "right",
			left: 300 + CURSOR_OFFSET,
			top: 200 - size.height / 2,
		});
	});

	it("falls back to the left when the right side would overflow", () => {
		// Right edge would land at 900 + 14 + 120 = 1034 > 992.
		const p = placeCursorTooltip({ x: 900, y: 200 }, size, viewport);
		expect(p.side).toBe("left");
		expect(p.left).toBe(900 - CURSOR_OFFSET - size.width);
		expect(p.top).toBe(200 - size.height / 2);
	});

	it("still uses the right side when the box exactly fits", () => {
		// left = x + 14; the box fits when left + width <= 992.
		const x =
			viewport.width - VIEWPORT_PADDING - size.width - CURSOR_OFFSET;
		expect(placeCursorTooltip({ x, y: 200 }, size, viewport).side).toBe(
			"right",
		);
		expect(
			placeCursorTooltip({ x: x + 1, y: 200 }, size, viewport).side,
		).toBe("left");
	});

	it("falls back to below when neither side fits", () => {
		const narrow = { width: 200, height: 600 };
		// 100 + 14 + 120 overflows the right; 100 - 14 - 120 < 8 overflows the left.
		const p = placeCursorTooltip({ x: 100, y: 200 }, size, narrow);
		expect(p.side).toBe("bottom");
		expect(p.top).toBe(200 + CURSOR_OFFSET);
		// Horizontally centered on the cursor: 100 - 60 = 40.
		expect(p.left).toBe(40);
	});

	it("clamps the centered box inside the top and bottom padding", () => {
		const nearTop = placeCursorTooltip({ x: 300, y: 5 }, size, viewport);
		expect(nearTop.top).toBe(VIEWPORT_PADDING);

		const nearBottom = placeCursorTooltip(
			{ x: 300, y: 598 },
			size,
			viewport,
		);
		expect(nearBottom.top).toBe(
			viewport.height - VIEWPORT_PADDING - size.height,
		);
	});

	it("clamps the below fallback horizontally", () => {
		// 130px viewport: right needs 10+14+120=144 > 122, left needs
		// 10-14-120 < 8, so it drops below; centering would put left at -50.
		const tiny = { width: 130, height: 600 };
		const p = placeCursorTooltip({ x: 10, y: 200 }, size, tiny);
		expect(p.side).toBe("bottom");
		expect(p.left).toBe(VIEWPORT_PADDING);
	});
});
