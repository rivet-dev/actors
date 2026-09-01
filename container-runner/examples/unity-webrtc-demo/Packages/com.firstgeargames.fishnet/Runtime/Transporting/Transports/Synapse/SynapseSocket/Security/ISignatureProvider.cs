using System.Net;

namespace SynapseSocket.Security
{

    /// <summary>
    /// Intermediate interface defining how connection signatures are calculated.
    /// Implementations should derive a stable identifier for a remote peer from properties such as its <see cref="IPEndPoint"/>, device info, or handshake payload.
    /// Signatures are used for identity, blacklisting, and spoofing mitigation.
    /// </summary>
    public interface ISignatureProvider
    {
        /// <summary>
        /// Computes a signature for the remote endpoint. The same endpoint should produce the same signature across calls.
        /// </summary>
        /// <param name="endPoint">The remote peer's endpoint.</param>
        /// <param name="handshakePayload">
        /// The handshake payload bytes, or empty if not yet available.
        /// The payload contains a unique nonce per handshake and may be incorporated into the signature for custom identity schemes.
        /// The engine independently mixes the nonce into the handshake replay cache key, so implementations do not need to include <paramref name="handshakePayload"/> to preserve replay protection.
        /// </param>
        /// <param name="signature">A produced 64-bit signature.</param>
        /// <returns>True if a signature was produced without error.</returns>
        bool TryCompute(IPEndPoint endPoint, System.ReadOnlySpan<byte> handshakePayload, out ulong signature);
    }
}
