import * as jose from "jose";

export interface VerifyJwtOptions {
	/**
	 * The JWKS URI to fetch the public keys from.
	 * E.g., "https://example.com/.well-known/jwks.json"
	 */
	jwksUri: string;
	/**
	 * Expected issuer of the JWT.
	 */
	issuer?: string | string[];
	/**
	 * Expected audience of the JWT.
	 */
	audience?: string | string[];
	/**
	 * Allowed clock tolerance in seconds. Default is 0.
	 */
	clockTolerance?: number;
	/**
	 * Additional options to pass to `jose.jwtVerify`.
	 */
	algorithms?: string[];
}

const jwksCache = new Map<string, ReturnType<typeof jose.createRemoteJWKSet>>();

function getJwks(jwksUri: string) {
	let jwks = jwksCache.get(jwksUri);
	if (!jwks) {
		jwks = jose.createRemoteJWKSet(new URL(jwksUri));
		jwksCache.set(jwksUri, jwks);
	}
	return jwks;
}

/**
 * Verifies a JSON Web Token (JWT) using the keys from the specified JWKS URI.
 * The JWKS result is automatically cached per URI to reduce overhead.
 *
 * @param token The raw JWT token string.
 * @param options Options for verifying the token, requiring at least a `jwksUri`.
 * @returns The verified JWT payload and protected header.
 */
export async function verifyJwt(
	token: string,
	options: VerifyJwtOptions,
): Promise<jose.JWTVerifyResult> {
	const JWKS = getJwks(options.jwksUri);

	return await jose.jwtVerify(token, JWKS, {
		issuer: options.issuer,
		audience: options.audience,
		clockTolerance: options.clockTolerance,
		algorithms: options.algorithms,
	});
}
