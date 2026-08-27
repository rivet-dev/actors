import { LogoMark } from "@/app/logo";

export function FullscreenLoading({
	children,
}: {
	children?: React.ReactNode;
}) {
	return (
		<div className="min-h-screen flex items-center justify-center flex-col bg-background text-foreground">
			<LogoMark className="h-10 w-10 animate-pulse" />
			{children}
		</div>
	);
}
