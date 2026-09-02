import { describe, expect, expectTypeOf, test, vi } from "vitest";
import { z } from "zod/v4";
import {
	flattenActionHandlers,
	flattenActionInputSchemas,
} from "../src/actor/actions";
import { type ActionContext, hasRunInspectorConfig } from "../src/actor/config";
import { actor, type BaseActorDefinition } from "../src/actor/definition";
import type { ActorDefinitionActions } from "../src/client/actor-common";
import type { ActorHandleRaw } from "../src/client/actor-handle";
import { createActorProxy } from "../src/client/client";
import type { RawAccess } from "../src/common/database/config";

describe("nested actions", () => {
	test("preserves nested handler context and client action types", () => {
		const definition = actor({
			state: { total: 0 },
			actions: {
				calculator: {
					add: (c, amount: number) => {
						c.state.total += amount;
						return c.state.total;
					},
				},
			},
		});
		type ClientActions = ActorDefinitionActions<typeof definition>;

		expectTypeOf<ClientActions["calculator"]["add"]>()
			.parameter(0)
			.toEqualTypeOf<number>();
		expectTypeOf<
			ClientActions["calculator"]["add"]
		>().returns.toEqualTypeOf<Promise<number>>();
	});

	test("preserves default database context and actions on base definitions", () => {
		const definition = actor({
			actions: {
				query: async (c, value: number) => {
					expectTypeOf(c.db).toEqualTypeOf<RawAccess>();
					return value.toString();
				},
			},
		});
		type DefinitionActions = ActorDefinitionActions<typeof definition>;
		expectTypeOf<DefinitionActions["query"]>()
			.parameter(0)
			.toEqualTypeOf<number>();

		type DefaultContext = ActionContext<
			undefined,
			undefined,
			undefined,
			undefined,
			undefined,
			undefined
		>;
		type DefaultActions = {
			query: (c: DefaultContext, value: number) => string;
		};
		type WorkflowStyleDefinition = BaseActorDefinition<
			undefined,
			undefined,
			undefined,
			undefined,
			undefined,
			undefined,
			Record<never, never>,
			Record<never, never>,
			DefaultActions
		>;
		type WorkflowStyleActions =
			ActorDefinitionActions<WorkflowStyleDefinition>;
		expectTypeOf<WorkflowStyleActions["query"]>()
			.parameter(0)
			.toEqualTypeOf<number>();
		expectTypeOf<WorkflowStyleActions["query"]>().returns.toEqualTypeOf<
			Promise<string>
		>();
	});

	test("detects legacy run inspector metadata without invoking its factory", () => {
		const inspectorFactory = vi.fn(() => undefined);
		const run = () => {};
		Object.defineProperty(run, Symbol.for("rivetkit.run_function_config"), {
			value: { inspectorFactory },
		});

		expect(hasRunInspectorConfig(run)).toBe(true);
		expect(inspectorFactory).not.toHaveBeenCalled();
	});

	test("dispatches nested proxy calls with dotted names", async () => {
		const action = vi.fn().mockResolvedValue("created");
		const handle = createActorProxy({
			action,
		} as unknown as ActorHandleRaw) as any;

		await expect(handle.users.create({ name: "Ada" })).resolves.toBe(
			"created",
		);
		expect(action).toHaveBeenCalledWith({
			name: "users.create",
			args: [{ name: "Ada" }],
		});
		expect(handle.users.then).toBeUndefined();
		expect(Object.getOwnPropertyDescriptor(handle, "then")).toBeUndefined();
	});

	test("preserves dotted namespace segments when matching nested schemas", () => {
		const create = () => "created";
		const schema = z.tuple([z.string()]);
		const actions = { "admin.users": { create } };

		expect(flattenActionHandlers(actions)).toEqual({
			"admin.users.create": create,
		});
		expect(
			flattenActionInputSchemas(actions, {
				"admin.users": { create: schema },
			}),
		).toEqual({ "admin.users.create": schema });
	});

	test("supports action names that overlap object prototype keys", () => {
		const prototypeAction = () => "prototype";
		const constructorAction = () => "constructor";
		const handlers = flattenActionHandlers({
			["__proto__"]: prototypeAction,
			constructor: constructorAction,
		});

		expect(Object.getPrototypeOf(handlers)).toBeNull();
		expect(handlers.__proto__).toBe(prototypeAction);
		expect(handlers.constructor).toBe(constructorAction);

		const definition = actor({
			actions: { ["__proto__"]: prototypeAction },
		});
		expect(Object.hasOwn(definition.config.actions, "__proto__")).toBe(
			true,
		);
	});

	test("flattens handlers and schemas to dotted names", () => {
		const create = () => "created";
		const schema = z.tuple([z.object({ name: z.string() })]);
		const actions = { users: { create } };

		expect(flattenActionHandlers(actions)).toEqual({
			"users.create": create,
		});
		expect(
			flattenActionInputSchemas(actions, {
				users: { create: schema },
			}),
		).toEqual({ "users.create": schema });
	});

	test("keeps dotted flat action and schema keys compatible", () => {
		const create = () => "created";
		const schema = z.tuple([z.string()]);
		const actions = { "users.create": create };

		expect(flattenActionHandlers(actions)).toEqual({
			"users.create": create,
		});
		expect(
			flattenActionInputSchemas(actions, { "users.create": schema }),
		).toEqual({ "users.create": schema });
	});

	test("rejects colliding flattened names", () => {
		expect(() =>
			flattenActionHandlers({
				"users.create": () => "flat",
				users: { create: () => "nested" },
			}),
		).toThrow("Multiple action definitions flatten to `users.create`");
	});

	test("rejects non-function leaves", () => {
		expect(() => flattenActionHandlers({ users: { create: 42 } })).toThrow(
			"Action `users.create` must be an action handler or group",
		);
	});

	test("limits the number of flattened actions", () => {
		const actionEntries = Array.from(
			{ length: 129 },
			(_, index) => [`action${index}`, () => index] as const,
		);
		const actions = Object.fromEntries(actionEntries);

		expect(() =>
			actor({ actions: Object.fromEntries(actionEntries.slice(0, 128)) }),
		).not.toThrow();
		expect(() => actor({ actions })).toThrow(
			"Actor defines 129 actions, but maxActions is 128",
		);
		expect(() =>
			actor({ actions, options: { maxActions: 129 } }),
		).not.toThrow();
	});
});
