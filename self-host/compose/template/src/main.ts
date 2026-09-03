#!/usr/bin/env node

import * as fs from "node:fs";
import * as path from "node:path";
import { TEMPLATES, type TemplateConfig } from "./config";
import { TemplateContext } from "./context";
import { generateDockerCompose } from "./docker-compose";
import { generateGitAttributes } from "./git";
import { generateReadme } from "./readme";
import { generateCoreClickhouse } from "./services/core/clickhouse";
import { generateCoreGrafana } from "./services/core/grafana";
import { generateCorePrometheus } from "./services/core/prometheus";
import { generateDatacenterOtelCollector } from "./services/edge/otel-collector";
import { generateDatacenterPostgres } from "./services/edge/postgres";
import { generateDatacenterRivetEngine } from "./services/edge/rivet-engine";
import { generateRunner } from "./services/edge/runner";
import { generateDatacenterVectorClient } from "./services/edge/vector-client";
import { generateDatacenterVectorServer } from "./services/edge/vector-server";

function generateTemplate(templateName: string, config: TemplateConfig) {
	const outputDir = path.join(__dirname, "../../", templateName);

	// Remove existing directory if it exists
	if (fs.existsSync(outputDir)) {
		fs.rmSync(outputDir, { recursive: true, force: true });
	}

	// Create directories
	if (!fs.existsSync(outputDir)) {
		fs.mkdirSync(outputDir, { recursive: true });
	}

	const context = new TemplateContext(config, outputDir);

	// Generate core services
	generateCoreClickhouse(context);
	generateCoreGrafana(context);
	generateCorePrometheus(context);

	// Generate datacenter-specific configurations
	for (const datacenter of config.datacenters) {
		generateDatacenterPostgres(context, datacenter.name);
		generateDatacenterRivetEngine(context, datacenter);
		generateDatacenterVectorServer(context, datacenter.name);
		generateDatacenterVectorClient(context, datacenter.name);
		generateDatacenterOtelCollector(context, datacenter);
	}

	generateRunner(context);
	generateDockerCompose(context);
	generateReadme(context, templateName);
	generateGitAttributes(context);

	console.log(`✅ Generated Docker Compose template: ${templateName}`);
}

function main() {
	for (const [templateName, template] of Object.entries(TEMPLATES)) {
		generateTemplate(templateName, template);
	}
}

if (require.main === module) {
	main();
}
