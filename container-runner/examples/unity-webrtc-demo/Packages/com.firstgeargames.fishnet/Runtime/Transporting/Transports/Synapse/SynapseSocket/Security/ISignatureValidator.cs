using System;
using System.Net;

namespace SynapseSocket.Security
{

    /// <summary>
    /// Optional interface for custom handshake validation.
    /// When supplied on <c>SynapseConfig.SignatureValidator</c>, the engine calls this for every incoming handshake and rejects the peer (raising <c>ConnectionFailed(SignatureRejected)</c>) if the validator returns false.
    /// Use this to enforce allow-lists, token schemes, or any application-specific identity check layered on top of the raw <see cref="ISignatureProvider"/> signature.
    /// </summary>
    public interface ISignatureValidator
    {
        /// <summary>
        /// Returns true to accept the handshake, false to reject it.
        /// </summary>
        /// <param name="endPoint">The remote peer's endpoint.</param>
        /// <param name="signature">The signature computed by the configured <see cref="ISignatureProvider"/>.</param>
        /// <param name="handshakePayload">The handshake payload bytes (may be empty).</param>
        /// <returns>True to accept the handshake; false to reject it and raise <c>ConnectionFailed(SignatureRejected)</c>.</returns>
        bool Validate(IPEndPoint endPoint, ulong signature, ReadOnlySpan<byte> handshakePayload);
    }
}
