using System;
using System.Collections.Concurrent;
using System.Net;
using SynapseSocket.Connections;
using SynapseSocket.Core.Configuration;

namespace SynapseSocket.Security
{

    /// <summary>
    /// Handles signature calculation, verification, and blacklisting for the SynapseSocket engine.
    /// Also enforces lowest-level mitigation rules such as per-endpoint packet frequency limits.
    /// </summary>
    public sealed class SecurityProvider
    {
        /// <summary>
        /// The signature calculator in use.
        /// </summary>
        public ISignatureProvider SignatureProvider { get; }
        /// <summary>
        /// Set of blacklisted peer signatures. Values are unused placeholders.
        /// </summary>
        private readonly ConcurrentDictionary<ulong, byte> _blacklist = new();
        /// <summary>
        /// Maximum number of packets a single peer may send per second. Zero disables packet rate limiting.
        /// </summary>
        private readonly uint _maximumPacketsPerSecond;
        /// <summary>
        /// Maximum number of bytes a single peer may send per second. Zero disables bytes rate limiting.
        /// </summary>
        private readonly uint _maximumBytesPerSecond;
        /// <summary>
        /// Maximum permitted size of a single incoming packet in bytes. Packets exceeding this are rejected.
        /// </summary>
        private readonly uint _maximumPacketSize;
        /// <summary>
        /// The sentinel value returned when a signature cannot be computed.
        /// The engine never blacklists this value.
        /// </summary>
        public const ulong UnsetSignature = 0;

        /// <summary>
        /// Creates a new security provider.
        /// </summary>
        /// <param name="signatureProvider">The signature provider used to identify remote peers.</param>
        /// <param name="maximumPacketsPerSecond">Per-peer packet rate limit. Zero disables packet rate limiting.</param>
        /// <param name="maximumBytesPerSecond">Per-peer byte rate limit. Zero disables byte rate limiting.</param>
        /// <param name="maximumPacketSize">Maximum permitted packet size in bytes.</param>
        public SecurityProvider(ISignatureProvider signatureProvider, uint maximumPacketsPerSecond, uint maximumBytesPerSecond, uint maximumPacketSize)
        {
            SignatureProvider = signatureProvider ?? throw new ArgumentNullException(nameof(signatureProvider));
            _maximumPacketsPerSecond = maximumPacketsPerSecond;
            _maximumBytesPerSecond = maximumBytesPerSecond;
            _maximumPacketSize = maximumPacketSize;
        }

        /// <summary>
        /// Computes the signature for an endpoint.
        /// Returns <see cref="UnsetSignature"/> if the provider reports failure.
        /// </summary>
        /// <param name="endPoint">The remote peer's endpoint.</param>
        /// <param name="handshakePayload">The handshake payload bytes, or empty if not yet available.</param>
        /// <returns>The computed 64-bit signature, or <see cref="UnsetSignature"/> on failure.</returns>
        public ulong ComputeSignature(IPEndPoint endPoint, ReadOnlySpan<byte> handshakePayload)
        {
            if (!SignatureProvider.TryCompute(endPoint, handshakePayload, out ulong signature))
                return UnsetSignature;

            return signature;
        }

        /// <summary>
        /// Returns true if the given signature is blacklisted.
        /// </summary>
        /// <param name="signature">The peer signature to check.</param>
        /// <returns>True if the signature is present in the blacklist.</returns>
        public bool IsBlacklisted(ulong signature) => _blacklist.ContainsKey(signature);

        /// <summary>
        /// Adds a signature to the blacklist.
        /// </summary>
        /// <param name="signature">The peer signature to blacklist.</param>
        public void AddToBlacklist(ulong signature) => _blacklist[signature] = 1;

        /// <summary>
        /// Removes a signature from the blacklist.
        /// Returns true if the signature was present and removed.
        /// </summary>
        /// <param name="signature">The peer signature to remove.</param>
        /// <returns>True if the signature was present and has been removed; false if it was not found.</returns>
        public bool RemoveFromBlacklist(ulong signature) => _blacklist.TryRemove(signature, out _);

        internal FilterResult InspectEstablished(SynapseConnection synapseConnection, int packetLength)
        {
            if (packetLength <= 0 || packetLength > _maximumPacketSize)
                return FilterResult.Oversized;

            // Packet-count and byte-count caps run as paired per-receive checks against
            // counters that the maintenance loop resets once per second. They catch two
            // distinct abuse shapes: packet floods (many tiny packets) and bandwidth floods
            // (fewer but larger packets that stay under the pps cap).
            if (_maximumPacketsPerSecond is not SecurityConfig.DisabledMaximumPacketsPerSecond && !synapseConnection.AllowReceivePacket(_maximumPacketsPerSecond))
                return FilterResult.RateLimited;

            if (_maximumBytesPerSecond is not SecurityConfig.DisabledMaximumBytesPerSecond && !synapseConnection.AllowReceiveBytes(packetLength, _maximumBytesPerSecond))
                return FilterResult.RateLimited;

            return FilterResult.Allowed;
        }

        /// <summary>
        /// Lowest-level filter for packets from an unknown or not-yet-established sender.
        /// Computes the signature (so violation reports carry the correct peer identity even for rejected packets), checks the blacklist, then delegates to <see cref="InspectEstablished"/> for size and rate-limit enforcement.
        /// </summary>
        /// <param name="nowTicks">Current timestamp in <see cref="System.DateTime.Ticks"/>.</param>
        /// <param name="endPoint">The remote endpoint the packet arrived from.</param>
        /// <param name="packetLength">Length of the received packet in bytes.</param>
        /// <param name="signature">The computed peer signature, or <see cref="UnsetSignature"/> on failure.</param>
        /// <returns>A <see cref="FilterResult"/> indicating whether the packet should be processed or dropped.</returns>
        public FilterResult InspectNew(IPEndPoint endPoint, int packetLength, out ulong signature)
        {
            // Reject immediately if the signature cannot be computed or resolves to the unset sentinel.
            // Without a valid identity we cannot rate-limit, blacklist, or attribute a violation correctly, so there is nothing useful we can do with the packet.
            if (!SignatureProvider.TryCompute(endPoint, ReadOnlySpan<byte>.Empty, out signature) || signature == UnsetSignature)
            {
                signature = UnsetSignature;
                return FilterResult.SignatureFailure;
            }

            if (_blacklist.ContainsKey(signature))
                return FilterResult.Blacklisted;

            if (packetLength <= 0 || packetLength > _maximumPacketSize)
                return FilterResult.Oversized;

            return FilterResult.Allowed;
        }
        
    }
}
