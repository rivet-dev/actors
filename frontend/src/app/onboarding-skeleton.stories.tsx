import type { Story } from "@ladle/react";
import "../../.ladle/ladle.css";
import { OnboardingSkeleton } from "./onboarding-skeleton";

export const Default: Story = () => (
	<div className="bg-background text-foreground">
		<OnboardingSkeleton />
	</div>
);

// The route pending components pass the real `SidebarlessHeader` so the header
// does not swap when the wizard mounts; the header needs a router, so this
// stands in for it.
export const WithCustomHeader: Story = () => (
	<div className="bg-background text-foreground">
		<OnboardingSkeleton
			header={
				<header className="z-20 flex items-center gap-2 h-12 px-3 shrink-0 border-b border-border bg-background text-sm">
					<div className="size-6 rounded-md bg-foreground/10" />
					<span className="text-muted-foreground">
						acme / production
					</span>
				</header>
			}
		/>
	</div>
);
