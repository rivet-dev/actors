using System;
using System.Buffers;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using SynapseSocket.Packets;

namespace SynapseSocket.Transport
{

    /// <summary>
    /// Transmission Engine (Sender).
    /// Manages outgoing packet flow for both the unreliable and reliable channels.
    /// Immediate processing (no batching) per the spec's no-batching policy.
    /// </summary>
    public sealed partial class TransmissionEngine
    {
        /// <summary>
        /// Sends a minimal NAT probe (no payload) to open a NAT mapping on the remote side.
        /// </summary>
        public Task SendNatProbeAsync(IPEndPoint target, CancellationToken cancellationToken)
        {
            const PacketType Type = PacketType.NatProbe;

            int headerSize = PacketHeader.ComputeHeaderSize(Type);
            byte[] rentedBuffer = ArrayPool<byte>.Shared.Rent(headerSize);

            PacketHeader.Write(rentedBuffer.AsSpan(), Type, 0, 0, 0, 0);
            return SendAndPoolBufferAsync(new(rentedBuffer, 0, headerSize), target, cancellationToken);
        }

        /// <summary>
        /// Sends a NAT challenge or challenge echo containing the provided token.
        /// Used both when issuing a challenge (server → initiator) and when echoing one (initiator → server).
        /// </summary>
        public Task SendNatChallengeAsync(IPEndPoint target, ReadOnlySpan<byte> token, CancellationToken cancellationToken)
        {
            const PacketType Type = PacketType.NatChallenge;

            int headerSize = PacketHeader.ComputeHeaderSize(Type);
            int totalSize = headerSize + token.Length;
            byte[] rentedBuffer = ArrayPool<byte>.Shared.Rent(totalSize);

            PacketHeader.Write(rentedBuffer.AsSpan(), Type, 0, 0, 0, 0);
            token.CopyTo(rentedBuffer.AsSpan(headerSize));

            return SendAndPoolBufferAsync(new(rentedBuffer, 0, totalSize), target, cancellationToken);
        }
    }
}
