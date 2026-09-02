import { describe, expect, test } from "vitest";
import {
	ActorOptionsSchema,
	type DefineRunHandlerOptions,
	defineRunHandler,
	getRunFunction,
	getRunInspectorKind,
	getRunMetadata,
} from "./config";

describe("ActorOptionsSchema", () => {
	test("keeps the Actor Runtime Socket opt-in", () => {
		expect(ActorOptionsSchema.parse({}).enableActorRuntimeSocket).toBe(
			false,
		);
		expect(
			ActorOptionsSchema.parse({ enableActorRuntimeSocket: true })
				.enableActorRuntimeSocket,
		).toBe(true);
	});

	test("defaults the action limit to 128 and accepts overrides", () => {
		expect(ActorOptionsSchema.parse({}).maxActions).toBe(128);
		expect(ActorOptionsSchema.parse({ maxActions: 256 }).maxActions).toBe(
			256,
		);
		expect(() => ActorOptionsSchema.parse({ maxActions: -1 })).toThrow();
	});
});

describe("defineRunHandler", () => {
	test("preserves the callable and exposes static metadata without creating an inspector", () => {
		let inspectorCreates = 0;
		const run = async (value: number): Promise<string> => String(value);
		const defined = defineRunHandler(run, {
			name: "Durable import",
			icon: "diagram-project",
			inspectorKind: "workflow",
			createInspector: () => {
				inspectorCreates += 1;
				return {
					inspector: {
						workflow: {
							getHistory: () => null,
							getState: async () => null,
							onHistoryUpdated: () => () => {},
							replayFromStep: async () => null,
						},
					},
				};
			},
		});

		const exactType: (value: number) => Promise<string> = defined;
		expect(exactType).toBe(run);
		expect(getRunFunction(defined)).toBe(run);
		expect(getRunMetadata(defined)).toEqual({
			name: "Durable import",
			icon: "diagram-project",
		});
		expect(getRunInspectorKind(defined)).toBe("workflow");
		expect(inspectorCreates).toBe(0);
	});

	test("requires static inspector metadata and its factory together", () => {
		expect(() =>
			defineRunHandler(async () => {}, {
				inspectorKind: "workflow",
			} as unknown as DefineRunHandlerOptions),
		).toThrow("requires inspectorKind and createInspector together");
		expect(() =>
			defineRunHandler(async () => {}, {
				createInspector: () => ({
					inspector: {
						workflow: {
							getHistory: () => null,
							getState: async () => null,
							onHistoryUpdated: () => () => {},
							replayFromStep: async () => null,
						},
					},
				}),
			} as unknown as DefineRunHandlerOptions),
		).toThrow("requires inspectorKind and createInspector together");
	});
});
