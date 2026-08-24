import { useMemo } from "react";
import { useEndpoint } from "@/app/dialogs/connect-manual-serverful-frame";
import { useRivetDsn } from "@/app/env-variables";

export function useRailwayTemplateLink({ runnerName }: { runnerName: string }) {
	const endpoint = useEndpoint();
	const secretDsn = useRivetDsn({ endpoint, kind: "secret" });
	const publicDsn = useRivetDsn({ endpoint, kind: "publishable" });

	return useMemo(() => {
		const url = new URL(
			"https://railway.com/new/template/rivet-cloud-starter",
		);
		url.searchParams.set("referralCode", "RC7bza");
		url.searchParams.set("utm_medium", "integration");
		url.searchParams.set("utm_source", "template");
		url.searchParams.set("utm_campaign", "generic");

		url.searchParams.set("RIVET_POOL", runnerName || "");
		if (secretDsn) {
			url.searchParams.set("RIVET_ENDPOINT", secretDsn);
		}
		if (publicDsn) {
			url.searchParams.set("RIVET_PUBLIC_ENDPOINT", publicDsn);
		}

		return url.toString();
	}, [runnerName, secretDsn, publicDsn]);
}
