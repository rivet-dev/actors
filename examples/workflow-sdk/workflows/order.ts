import { sleep } from "workflow";

export async function processOrder(id: string) {
	"use workflow";

	const reserved = await reserveInventory(id);
	await sleep("1 hour");
	return chargeOrder(reserved);
}

async function reserveInventory(id: string) {
	"use step";
	return { id, reservationId: `reservation-${id}` };
}

async function chargeOrder(order: { id: string; reservationId: string }) {
	"use step";
	return { ...order, status: "charged" as const };
}
