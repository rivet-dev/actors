import { serve } from "@hono/node-server";
import { Hono } from "hono";
import * as jose from "jose";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { verifyJwt } from "../../src/auth/jwt";

describe("verifyJwt", () => {
	let jwksServer: ReturnType<typeof serve>;
	let jwksUrl: string;
	let privateKey: jose.KeyLike;
	let publicKey: jose.KeyLike;
	let keyId = "test-key-1";

	beforeAll(async () => {
		// Generate an RSA key pair for testing
		const { publicKey: pub, privateKey: priv } =
			await jose.generateKeyPair("RS256");
		publicKey = pub;
		privateKey = priv;

		// Export the public key to JWK format
		const jwk = await jose.exportJWK(publicKey);
		jwk.kid = keyId;
		jwk.alg = "RS256";
		jwk.use = "sig";

		// Create a mock JWKS server
		const app = new Hono();
		app.get("/.well-known/jwks.json", (c) => {
			return c.json({ keys: [jwk] });
		});

		return new Promise((resolve) => {
			jwksServer = serve(
				{
					fetch: app.fetch,
					port: 0, // dynamic port
				},
				(info) => {
					jwksUrl = `http://localhost:${info.port}/.well-known/jwks.json`;
					resolve();
				},
			);
		});
	});

	afterAll(() => {
		if (jwksServer) {
			jwksServer.close();
		}
	});

	test("successfully verifies a valid JWT", async () => {
		const token = await new jose.SignJWT({ userId: "123" })
			.setProtectedHeader({ alg: "RS256", kid: keyId })
			.setIssuedAt()
			.setIssuer("https://example.com")
			.setAudience("my-client")
			.setExpirationTime("2h")
			.sign(privateKey);

		const result = await verifyJwt(token, {
			jwksUri: jwksUrl,
			issuer: "https://example.com",
			audience: "my-client",
		});

		expect(result.payload.userId).toBe("123");
		expect(result.protectedHeader.kid).toBe(keyId);
	});

	test("fails if token has wrong issuer", async () => {
		const token = await new jose.SignJWT({ userId: "123" })
			.setProtectedHeader({ alg: "RS256", kid: keyId })
			.setIssuedAt()
			.setIssuer("https://wrong-issuer.com")
			.setAudience("my-client")
			.setExpirationTime("2h")
			.sign(privateKey);

		await expect(
			verifyJwt(token, {
				jwksUri: jwksUrl,
				issuer: "https://example.com",
				audience: "my-client",
			}),
		).rejects.toThrow("unexpected \"iss\" claim value");
	});

	test("fails if token has expired", async () => {
		const token = await new jose.SignJWT({ userId: "123" })
			.setProtectedHeader({ alg: "RS256", kid: keyId })
			.setIssuedAt()
			.setIssuer("https://example.com")
			.setExpirationTime("-1h") // Expired 1 hour ago
			.sign(privateKey);

		await expect(
			verifyJwt(token, {
				jwksUri: jwksUrl,
				issuer: "https://example.com",
			}),
		).rejects.toThrow(/\"exp\" claim timestamp check failed/);
	});
});
