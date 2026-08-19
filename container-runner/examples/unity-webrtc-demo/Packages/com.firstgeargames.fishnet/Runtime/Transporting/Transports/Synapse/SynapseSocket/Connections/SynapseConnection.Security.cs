using System;
using CodeBoost.CodeAnalysis;
using SynapseSocket.Core.Configuration;

namespace SynapseSocket.Connections
{

    /// <summary>
    /// Represents the state of a single remote peer session, including reliable send/receive windows, keep-alive timestamps, and signature binding.
    /// </summary>
    public sealed partial class SynapseConnection
    {

        /// <summary>
        /// Number of packets received by this connection since <see cref="_inboundRateCountersResetTick"/> was last set.
        /// Incremented on every inbound packet and cleared by <see cref="ResetInboundRateCounters"/>.
        /// </summary>
        [PoolResettableMember]
        private int _receivedByPacketCount;
        /// <summary>
        /// Number of bytes received by this connection since <see cref="_inboundRateCountersResetTick"/> was last set.
        /// Incremented by each inbound packet's length and cleared by <see cref="ResetInboundRateCounters"/>.
        /// </summary>
        [PoolResettableMember]
        private int _receivedByBytesCount;
        /// <summary>
        /// UTC ticks of the last time the inbound rate counters were reset to zero.
        /// Used to enforce the one-second window for per-connection rate limiting across both
        /// <see cref="_receivedByPacketCount"/> and <see cref="_receivedByBytesCount"/>.
        /// </summary>
        [PoolResettableMember]
        private long _inboundRateCountersResetTick;

        /// <summary>
        /// Increments the inbound packet counter and returns whether the connection is within the allowed rate.
        /// Returns false when <see cref="_receivedByPacketCount"/> exceeds <paramref name="maximumPacketsPerSecond"/>.
        /// </summary>
        /// <param name="maximumPacketsPerSecond">The per-connection packet rate cap for the current second.</param>
        /// <returns>True if the packet should be processed; false if it should be dropped.</returns>
        internal bool AllowReceivePacket(uint maximumPacketsPerSecond)
        {
            if (++_receivedByPacketCount > maximumPacketsPerSecond)
                return false;

            return true;
        }

        /// <summary>
        /// Adds <paramref name="packetLength"/> to the inbound byte counter and returns whether the
        /// connection is within the allowed rate. Returns false when
        /// <see cref="_receivedByBytesCount"/> exceeds <paramref name="maximumBytesPerSecond"/>.
        /// </summary>
        /// <param name="packetLength">Size of the inbound packet in bytes (assumed non-negative and already validated by the oversize check).</param>
        /// <param name="maximumBytesPerSecond">The per-connection byte rate cap for the current second.</param>
        /// <returns>True if the packet should be processed; false if it should be dropped.</returns>
        internal bool AllowReceiveBytes(int packetLength, uint maximumBytesPerSecond)
        {
            _receivedByBytesCount += packetLength;
        
            if (_receivedByBytesCount > maximumBytesPerSecond)
                return false;

            return true;
        }

        /// <summary>
        /// Resets both <see cref="_receivedByPacketCount"/> and <see cref="_receivedByBytesCount"/>
        /// to zero once per second. Has no effect if called within the same one-second window as the
        /// previous reset.
        /// </summary>
        /// <param name="nowTicks">Current UTC ticks, used to determine whether the one-second window has elapsed.</param>
        internal void ResetInboundRateCounters(long nowTicks)
        {
            if (nowTicks - _inboundRateCountersResetTick < TimeSpan.TicksPerSecond)
                return;

            _inboundRateCountersResetTick = nowTicks;
            _receivedByPacketCount = 0;
            _receivedByBytesCount = 0;
        }
    }
}
