import { toNextHandler } from "@rivetkit/next-js";
import { registry } from "@/rivet/registry";

export const maxDuration = 300;

export const { GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS } = toNextHandler(registry);
