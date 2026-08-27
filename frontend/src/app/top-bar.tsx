import { Link } from "@tanstack/react-router";
import { cn } from "@/components";
import { ContextSwitcher } from "./context-switcher";
import { LogoMark } from "./logo";
import { TopBarActions } from "./top-bar-actions";

export function TopBar() {
	return (
		<header
			className={cn(
				"z-20 flex items-center gap-2 h-12 px-3 shrink-0",
				"border-b border-border bg-background",
			)}
		>
			<TopBarLogo />
			<ContextSwitcher inline />
			<TopBarActions />
		</header>
	);
}

function TopBarLogo() {
	return (
		<Link to="/" className="flex items-center shrink-0 pr-1">
			<LogoMark className="h-6 w-6" />
		</Link>
	);
}
