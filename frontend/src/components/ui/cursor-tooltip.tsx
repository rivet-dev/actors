"use client";

import { AnimatePresence, motion, useSpring } from "framer-motion";
import {
	createContext,
	type FocusEvent,
	type PointerEvent,
	type ReactNode,
	useContext,
	useId,
	useLayoutEffect,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/utils";
import { type Point, placeCursorTooltip } from "./cursor-tooltip-placement";
import { Kbd } from "./kbd";

// A single tooltip shared by a group of triggers. Instead of anchoring to each
// trigger, it trails the cursor: it appears just to the right of the pointer
// and springs after it, swapping its label as the pointer moves between
// triggers. When the right side doesn't fit it falls back to the left, then
// below (see cursor-tooltip-placement.ts). Keyboard focus anchors it to the
// right edge of the focused trigger instead.

// Short grace period so moving between adjacent triggers glides the tooltip
// instead of hiding and re-showing it.
const HIDE_DELAY_MS = 80;

type Active = { id: string; content: ReactNode };

interface CursorTooltipContextValue {
	show: (id: string, content: ReactNode, point: Point) => void;
	move: (point: Point) => void;
	hide: (id: string, opts?: { immediate?: boolean }) => void;
}

const CursorTooltipContext = createContext<CursorTooltipContextValue | null>(
	null,
);

const SPRING = { stiffness: 600, damping: 45, mass: 0.6 };

export function CursorTooltipGroup({
	children,
	className,
}: {
	children: ReactNode;
	className?: string;
}) {
	const [active, setActive] = useState<Active | null>(null);
	const activeRef = useRef<Active | null>(null);
	const hideTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
	const lastPoint = useRef<Point | null>(null);
	const snapOnMeasure = useRef(false);
	const contentRef = useRef<HTMLDivElement>(null);
	const tooltipId = useId();

	const x = useSpring(0, SPRING);
	const y = useSpring(0, SPRING);

	const place = (point: Point, snap: boolean) => {
		lastPoint.current = point;
		const el = contentRef.current;
		const { left, top } = placeCursorTooltip(
			point,
			{ width: el?.offsetWidth ?? 0, height: el?.offsetHeight ?? 0 },
			{ width: window.innerWidth, height: window.innerHeight },
		);
		if (snap) {
			x.jump(left);
			y.jump(top);
		} else {
			x.set(left);
			y.set(top);
		}
	};

	// The tooltip's size is only known after it renders, so re-run placement
	// once the content is in the DOM. On first show this snaps into place so
	// the tooltip doesn't slide in from the unmeasured position. This drives
	// motion values, not React state.
	// biome-ignore lint/correctness/useExhaustiveDependencies: `place` reads refs and motion values only; rerun on `active` changes
	useLayoutEffect(() => {
		if (active && lastPoint.current) {
			place(lastPoint.current, snapOnMeasure.current);
			snapOnMeasure.current = false;
		}
	}, [active]);

	const clearHide = () => {
		if (hideTimeout.current) {
			clearTimeout(hideTimeout.current);
			hideTimeout.current = null;
		}
	};

	const show = (id: string, content: ReactNode, point: Point) => {
		clearHide();
		const isFirstShow = activeRef.current === null;
		snapOnMeasure.current = isFirstShow;
		place(point, isFirstShow);
		const next = { id, content };
		activeRef.current = next;
		setActive(next);
	};

	const move = (point: Point) => {
		if (activeRef.current) place(point, false);
	};

	const hide = (id: string, opts?: { immediate?: boolean }) => {
		const commit = () => {
			if (activeRef.current?.id !== id) return;
			activeRef.current = null;
			setActive(null);
		};
		clearHide();
		if (opts?.immediate) {
			commit();
			return;
		}
		hideTimeout.current = setTimeout(commit, HIDE_DELAY_MS);
	};

	return (
		<CursorTooltipContext.Provider value={{ show, move, hide }}>
			<div className={className}>{children}</div>
			{createPortal(
				<AnimatePresence>
					{active ? (
						<motion.div
							ref={contentRef}
							id={tooltipId}
							role="tooltip"
							// x/y position the box's top-left corner; the
							// placement math already centers it on the cursor.
							style={{ x, y }}
							initial={{ opacity: 0, scale: 0.95 }}
							animate={{ opacity: 1, scale: 1 }}
							exit={{ opacity: 0, scale: 0.95 }}
							transition={{ duration: 0.12 }}
							className={cn(
								"pointer-events-none fixed left-0 top-0 z-50 origin-left",
								"whitespace-nowrap rounded-md border border-foreground/10 bg-popover/95 backdrop-blur-md px-2.5 py-1.5 text-sm text-popover-foreground shadow-lg",
							)}
						>
							<AnimatePresence initial={false} mode="popLayout">
								<motion.span
									key={active.id}
									initial={{ opacity: 0, y: 4 }}
									animate={{ opacity: 1, y: 0 }}
									exit={{ opacity: 0, y: -4 }}
									transition={{ duration: 0.1 }}
									className="flex items-center gap-2"
								>
									{active.content}
								</motion.span>
							</AnimatePresence>
						</motion.div>
					) : null}
				</AnimatePresence>,
				document.body,
			)}
		</CursorTooltipContext.Provider>
	);
}

export function CursorTooltipTrigger({
	content,
	children,
	className,
	disabled,
}: {
	content: ReactNode;
	children: ReactNode;
	className?: string;
	disabled?: boolean;
}) {
	const ctx = useContext(CursorTooltipContext);
	const id = useId();

	if (!ctx) {
		throw new Error(
			"CursorTooltipTrigger must be rendered inside CursorTooltipGroup",
		);
	}

	if (disabled) {
		return <>{children}</>;
	}

	const pointOf = (e: PointerEvent) => ({ x: e.clientX, y: e.clientY });

	// Keyboard users have no cursor, so anchor to the trigger's right edge.
	const anchorOf = (e: FocusEvent<HTMLElement>) => {
		const rect = e.currentTarget.getBoundingClientRect();
		return { x: rect.right, y: rect.top + rect.height / 2 };
	};

	return (
		// biome-ignore lint/a11y/noStaticElementInteractions: pointer handlers on a passive wrapper so disabled buttons still surface a tooltip
		<span
			className={cn("inline-flex", className)}
			onPointerEnter={(e) => ctx.show(id, content, pointOf(e))}
			onPointerMove={(e) => ctx.move(pointOf(e))}
			onPointerLeave={() => ctx.hide(id)}
			onPointerDown={() => ctx.hide(id, { immediate: true })}
			onFocus={(e) => ctx.show(id, content, anchorOf(e))}
			onBlur={() => ctx.hide(id, { immediate: true })}
		>
			{children}
		</span>
	);
}

// Keyboard shortcut pill for tooltip content: `<>Search <CursorTooltipShortcut
// keys="K" /></>` renders "Search ⌘ K" (Ctrl on non-Mac). Pass `mod={false}`
// for shortcuts without the platform modifier.
export function CursorTooltipShortcut({
	keys,
	mod = true,
}: {
	keys: string;
	mod?: boolean;
}) {
	return (
		<Kbd className="h-5 gap-1 border-0 bg-foreground/10 px-1.5 font-sans text-xs font-medium text-muted-foreground">
			{mod ? <Kbd.Key className="text-xs" /> : null}
			<span>{keys}</span>
		</Kbd>
	);
}
