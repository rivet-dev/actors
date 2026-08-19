using System;
using System.Net;

namespace SynapseSocket.Security
{

    /// <summary>
    /// Default signature provider: hashes the remote IP address and port using an FNV-1a 64-bit hash.
    /// This binds the signature to the full endpoint, distinguishing multiple clients behind the same NAT address.
    /// </summary>
    public sealed class DefaultSignatureProvider : ISignatureProvider
    {
        /// <summary>
        /// FNV-1a 64-bit offset basis.
        /// </summary>
        private const ulong FnvOffset = 14695981039346656037UL;
        /// <summary>
        /// FNV-1a 64-bit prime multiplier.
        /// </summary>
        private const ulong FnvPrime = 1099511628211UL;

        /// <inheritdoc/>
        public bool TryCompute(IPEndPoint endPoint, ReadOnlySpan<byte> handshakePayload, out ulong signature)
        {
            if (endPoint is null)
            {
                signature = SecurityProvider.UnsetSignature;
                return false;
            }

            Span<byte> addressBytes = stackalloc byte[16];

            if (!endPoint.Address.TryWriteBytes(addressBytes, out int writtenByteCount))
            {
                signature = SecurityProvider.UnsetSignature;
                return false;
            }

            ulong hash = FnvOffset;

            for (int i = 0; i < writtenByteCount; i++)
            {
                hash ^= addressBytes[i];
                hash *= FnvPrime;
            }

            hash ^= (ulong)endPoint.Port;
            hash *= FnvPrime;

            signature = hash;
            return true;
        }
    }
}
